# Work plan: every open parity row in a slice, a queued brief, or a named list

Written by a planning reader on 2026-11-17 at HEAD `c5b6432ce` of branch `gpui` (worktree `agent-a307dd904b5ae1f01`). Nothing was built or run but Python scripts over the repository's files; every `app-gpui/` file was read with `git show HEAD:` (another agent is editing `app-gpui/src`). The scripts are in this folder: `count.py` (part 1), `callers.py` (who calls an IPC command or listens to an event), `slices.py` with `buckets_def.py` and `buckets_features*.py` (the assignment: an ordered list of buckets with match rules; the first matching bucket wins, catch-alls last), `write_plan.py` (the order and this file). `assignment.tsv` holds every row's bucket; `buckets.json` the per-slice data; `order.json` the order.

Every statement in this file that was not measured by a script or by reading the code at HEAD is marked Hypothesis. Sizes (lines to write) are a formula, so every size is a hypothesis: 0.35 × the webview implementation files the slice names or its specs are named after, 0.05 × a whole webview folder it names, + 18 per spec case still to prove, + per non-spec row 120 (no record, user-facing: action, overlay, tool window, editor kind, page, status or title item) or 60 (no record, other) or 30 (partial or model only); or a hand estimate (Vim: the design note's; I-UNTITLED 900, I-WINDOW-GROUPS 2,400, F-APP-NEW-PROJECT 1,600, F-DEPLOY-WINDOW 1,800, F-WIT 2,300, I-ZOOM-PX 2,600). Ranking factor 0.6 for a slice that needs a new native component: a hand list, or more than 70 % of its rows with no record (Hypothesis: the share of partial and model-only rows stands for existing native neighbours). `splits.py` cuts the larger buckets into writer-sized sub-slices by key pattern. Cases still to prove: every case of a row with no record, a third of a model-only row's (wiring), and for a partial row the count its proof gives ("N of M cases") or the re-verification reports list (`RETRIAGE_UNIT_1.md`, `RETRIAGE_UNIT_2.md`: 417 missing cases over 113 spec files, `missing_by_spec.txt`), else half.

`OWED.md` was read whole; of its entries only line 15 (`git_forms.rs` Debug: stale, see the contradictions) and line 21 (Restricted Mode writes: read, still a hypothesis) were checked against the code at HEAD; its other entries enter this plan through their parity rows or as items of `T1-SAFETY-CHECKS` and the not-planned list. Measured at the end of the work: HEAD moved from `eb1ac8c00` to `c5b6432ce` during the work (New from Template landed, D149); every count, the assignment and the order were re-run on `c5b6432ce`, whose `docs/` and `src/` equal the working tree (`git status --short docs src` prints nothing). The notes were read at `eb1ac8c00`; the four commits after it add to `backlog.md` and `OWED.md` (D149 done, follow upstream 3 running) and add the integrator's report `notes/reports/int_templates_files.md`, which `F-NFT-REST` names.

## 1. What is open, counted

Counted by `count.py` over `docs/gpui-port/INVENTORY.md` (a row's mark is the generator's, which reads `parity.json`; `[m]` is a `done` row whose cited tests reach no app code). Rows not at parity: **2580** = 1,676 with no record `[ ]` + 839 partial `[~]` + 65 model only `[m]`. By the brief's literal wording (not done in `parity.json`, or no record) the count is 2,515; the 65 `[m]` rows are `done` in `parity.json` but not at parity by the inventory's rule, so they are planned too. Folder: the feature folder of the row's source path; IPC rows and backend events are re-owned by `callers.py` to the webview folder that calls them most (`ipc/<adapter>` in the folder table below is the adapter file, the table in part 2 uses the caller).

| Section                                                | No record `[ ]` | Partial `[~]` | Model only `[m]` | Open |
| ------------------------------------------------------ | --------------- | ------------- | ---------------- | ---- |
| Behaviour specification: unit tests (vitest)           | 363             | 316           | 64               | 743  |
| Actions                                                | 323             | 214           | 0                | 537  |
| IPC commands                                           | 348             | 49            | 0                | 397  |
| Editor, platform, workbench and design-system modules  | 192             | 72            | 1                | 265  |
| Behaviour specification: end-to-end specs (Playwright) | 95              | 11            | 0                | 106  |
| Settings pages                                         | 16              | 38            | 0                | 54   |
| Overlays                                               | 26              | 26            | 0                | 52   |
| Context keys                                           | 44              | 6             | 0                | 50   |
| Features                                               | 28              | 19            | 0                | 47   |
| Plugin API interfaces (wit/)                           | 33              | 13            | 0                | 46   |
| Extension point `registerEditorContribution`           | 31              | 9             | 0                | 40   |
| Tool windows                                           | 12              | 22            | 0                | 34   |
| Extension point `registerMenuGroup`                    | 18              | 7             | 0                | 25   |
| Native shell modules (src-tauri)                       | 23              | 2             | 0                | 25   |
| Editor kinds                                           | 16              | 7             | 0                | 23   |
| Extension point `registerRunConfigType`                | 21              | 1             | 0                | 22   |
| Backend events (IdeEvent)                              | 12              | 8             | 0                | 20   |
| Status bar items                                       | 9               | 5             | 0                | 14   |
| Extension point `registerKeyClaim`                     | 9               | 1             | 0                | 10   |
| Extension point `registerEditorBanner`                 | 6               | 0             | 0                | 6    |
| Extension point `registerNavigationProvider`           | 6               | 0             | 0                | 6    |
| Extension point `registerProjectViewRoot`              | 5               | 0             | 0                | 5    |
| Extension point `registerFontZoomSurface`              | 4               | 0             | 0                | 4    |
| Extension point `registerRunExecutor`                  | 4               | 0             | 0                | 4    |
| Extension point `registerAgentContextSource`           | 3               | 0             | 0                | 3    |
| Extension point `registerServiceProvider`              | 2               | 1             | 0                | 3    |
| Extension point `registerTextEditorWrapper`            | 2               | 1             | 0                | 3    |
| Keymap ids with no statically registered action        | 2               | 0             | 0                | 2    |
| Keymap presets                                         | 0               | 2             | 0                | 2    |
| Extension point `registerFileDropHandler`              | 2               | 0             | 0                | 2    |
| File editor providers                                  | 1               | 1             | 0                | 2    |
| Extension point `registerRunAnythingProvider`          | 2               | 0             | 0                | 2    |
| Extension point `registerSaveParticipant`              | 2               | 0             | 0                | 2    |
| Extension point `registerServiceView`                  | 0               | 2             | 0                | 2    |
| Title bar items                                        | 0               | 2             | 0                | 2    |
| Extension point `registerCloneSource`                  | 1               | 0             | 0                | 1    |
| Extension point `registerCommitCheckProvider`          | 1               | 0             | 0                | 1    |
| Extension point `registerDocumentIndexSync`            | 1               | 0             | 0                | 1    |
| Extension point `registerEditorDropHandler`            | 1               | 0             | 0                | 1    |
| Extension point `registerEditorGroupPanel`             | 1               | 0             | 0                | 1    |
| Extension point `registerEndpointProvider`             | 1               | 0             | 0                | 1    |
| Extension point `registerExtensionBindings`            | 1               | 0             | 0                | 1    |
| Extension point `registerExternalFileOpenHandler`      | 1               | 0             | 0                | 1    |
| Extension point `registerGitLogChecks`                 | 1               | 0             | 0                | 1    |
| Extension point `registerLanguageMode`                 | 1               | 0             | 0                | 1    |
| Extension point `registerMenuSlotItem`                 | 0               | 1             | 0                | 1    |
| Extension point `registerOccurrenceNavigator`          | 0               | 1             | 0                | 1    |
| Extension point `registerPendingEdits`                 | 1               | 0             | 0                | 1    |
| Extension point `registerRunningProcessesProvider`     | 1               | 0             | 0                | 1    |
| Extension point `registerSoftWrapSurface`              | 1               | 0             | 0                | 1    |
| Extension point `registerUndoHandler`                  | 0               | 1             | 0                | 1    |
| Extension point `registerUriOpener`                    | 1               | 0             | 0                | 1    |
| Extension point `registerWorkbenchBanner`              | 0               | 1             | 0                | 1    |
| Extension point `registerShellActions`                 | 1               | 0             | 0                | 1    |
| Extension point `registerDevSamples`                   | 1               | 0             | 0                | 1    |

| Folder                 | No record | Partial | Model only | Open |
| ---------------------- | --------- | ------- | ---------- | ---- |
| features/vcs           | 87        | 99      | 9          | 195  |
| platform               | 79        | 53      | 2          | 134  |
| editor/core            | 67        | 58      | 3          | 128  |
| editor/lsp             | 71        | 49      | 7          | 127  |
| features/run           | 62        | 45      | 9          | 116  |
| features/settings      | 50        | 55      | 3          | 108  |
| e2e                    | 95        | 11      | 0          | 106  |
| features/database      | 66        | 37      | 0          | 103  |
| workbench              | 58        | 42      | 3          | 103  |
| features/debugger      | 69        | 21      | 0          | 90   |
| ui                     | 64        | 16      | 3          | 83   |
| features/agents        | 47        | 26      | 0          | 73   |
| features/plugins       | 42        | 24      | 4          | 70   |
| features/app           | 33        | 27      | 1          | 61   |
| wit                    | 33        | 13      | 0          | 46   |
| features/build-tools   | 31        | 11      | 0          | 42   |
| features/services      | 34        | 6       | 0          | 40   |
| ipc/plugin             | 32        | 8       | 0          | 40   |
| features/search        | 8         | 30      | 1          | 39   |
| features/github        | 26        | 12      | 1          | 39   |
| ipc/agent              | 31        | 7       | 0          | 38   |
| features/project-tree  | 14        | 19      | 1          | 34   |
| features/terminal      | 17        | 11      | 2          | 30   |
| ipc/lsp                | 29        | 1       | 0          | 30   |
| editor/refactor        | 20        | 8       | 0          | 28   |
| ipc/window             | 22        | 6       | 0          | 28   |
| src-tauri              | 23        | 2       | 0          | 25   |
| ipc/vcs                | 16        | 7       | 0          | 23   |
| features/http-client   | 12        | 9       | 0          | 21   |
| events                 | 12        | 8       | 0          | 20   |
| features/templates     | 2         | 17      | 0          | 19   |
| features/code-insight  | 3         | 16      | 0          | 19   |
| features/analysis      | 14        | 0       | 2          | 16   |
| features/dotnet        | 15        | 0       | 1          | 16   |
| ipc/docker             | 16        | 0       | 0          | 16   |
| editor/navigation      | 11        | 4       | 0          | 15   |
| features/viewers       | 6         | 9       | 0          | 15   |
| ipc/run                | 14        | 1       | 0          | 15   |
| features/macros        | 13        | 0       | 1          | 14   |
| features/coverage      | 13        | 0       | 1          | 14   |
| features/profiler      | 13        | 1       | 0          | 14   |
| ipc/app                | 7         | 7       | 0          | 14   |
| ipc/debug              | 14        | 0       | 0          | 14   |
| features/remote-dev    | 8         | 1       | 4          | 13   |
| features/vim           | 9         | 4       | 0          | 13   |
| features/graphql       | 10        | 1       | 1          | 12   |
| ipc/nav                | 12        | 0       | 0          | 12   |
| ipc/sdk                | 12        | 0       | 0          | 12   |
| features/deployment    | 2         | 9       | 0          | 11   |
| ipc/db                 | 11        | 0       | 0          | 11   |
| ipc/?                  | 11        | 0       | 0          | 11   |
| features/build         | 3         | 7       | 0          | 10   |
| features/tools         | 10        | 0       | 0          | 10   |
| features/perf          | 8         | 1       | 0          | 9    |
| features/web           | 9         | 0       | 0          | 9    |
| ipc/doc                | 8         | 1       | 0          | 9    |
| ipc/tools              | 9         | 0       | 0          | 9    |
| ipc/window_group       | 9         | 0       | 0          | 9    |
| features/notebook      | 2         | 6       | 0          | 8    |
| ipc/build              | 7         | 1       | 0          | 8    |
| ipc/heap               | 8         | 0       | 0          | 8    |
| features/project-trust | 0         | 7       | 0          | 7    |
| editor                 | 6         | 0       | 1          | 7    |
| ipc/clipboard          | 7         | 0       | 0          | 7    |
| features/scratch       | 6         | 0       | 0          | 6    |
| ipc/github             | 6         | 0       | 0          | 6    |
| ipc/mcp                | 6         | 0       | 0          | 6    |
| features/welcome       | 1         | 4       | 0          | 5    |
| features/libraries     | 4         | 1       | 0          | 5    |
| features/ssh           | 4         | 1       | 0          | 5    |
| features/color-preview | 3         | 2       | 0          | 5    |
| features/todo          | 1         | 4       | 0          | 5    |
| ipc/analysis           | 5         | 0       | 0          | 5    |
| ipc/fs                 | 4         | 1       | 0          | 5    |
| ipc/http               | 5         | 0       | 0          | 5    |
| features/whats-new     | 4         | 0       | 0          | 4    |
| features/run-anything  | 3         | 0       | 1          | 4    |
| ?                      | 2         | 2       | 0          | 4    |
| features/cucumber      | 4         | 0       | 0          | 4    |
| features/rails         | 4         | 0       | 0          | 4    |
| features/mcp           | 3         | 1       | 0          | 4    |
| features/logcat        | 2         | 0       | 2          | 4    |
| ipc/caches             | 2         | 2       | 0          | 4    |
| ipc/notebook           | 1         | 3       | 0          | 4    |
| ipc/scaffold           | 4         | 0       | 0          | 4    |
| ipc/ssh                | 3         | 1       | 0          | 4    |
| ipc/web                | 4         | 0       | 0          | 4    |
| ipc/workspace          | 4         | 0       | 0          | 4    |
| features/dependencies  | 2         | 0       | 1          | 3    |
| features/thymeleaf     | 3         | 0       | 0          | 3    |
| ipc/perf               | 3         | 0       | 0          | 3    |
| ipc/coverage           | 3         | 0       | 0          | 3    |
| ipc/java               | 2         | 1       | 0          | 3    |
| ipc/profiler           | 3         | 0       | 0          | 3    |
| styles                 | 3         | 0       | 0          | 3    |
| editor/structure       | 1         | 0       | 1          | 2    |
| ipc/android            | 2         | 0       | 0          | 2    |
| ipc/deploy             | 1         | 1       | 0          | 2    |
| ipc/file_dialog        | 2         | 0       | 0          | 2    |
| ipc/syntax             | 2         | 0       | 0          | 2    |
| ipc/spell              | 2         | 0       | 0          | 2    |
| ipc/fonts              | 2         | 0       | 0          | 2    |
| ipc/wsl                | 2         | 0       | 0          | 2    |
| ipc/dependencies       | 1         | 0       | 0          | 1    |
| ipc/env                | 1         | 0       | 0          | 1    |
| ipc/notification       | 1         | 0       | 0          | 1    |
| ipc/hotkeys            | 1         | 0       | 0          | 1    |
| ipc/term               | 1         | 0       | 0          | 1    |
| ipc/window_drag        | 0         | 1       | 0          | 1    |
| src/test               | 1         | 0       | 0          | 1    |

Coverage of those rows (measured by `slices.py`: every row lands in exactly one bucket):

| Bucket kind                                                      | Rows |
| ---------------------------------------------------------------- | ---- |
| already queued (briefs written, not planned again)               | 83   |
| a slice of this plan                                             | 2423 |
| cannot reach parity by a writer and an integrator alone (part 3) | 74   |
| not planned yet (named list with reason)                         | 0    |
| total                                                            | 2580 |

Queued (not planned again; the rows a brief names or whose every missing clause a brief item names, read in `parity.json` for 60 of them; a row with clauses beyond the brief is in a slice of this plan with the brief as a dependency):

- **Q-FU3** Follow upstream 3: MCP secrets on Export/Import, other-share check, small backend-call fixes (`docs/gpui-port/notes/reports/int_follow_upstream3_brief.md`): 15 rows. `ovl:workbench.backgroundTasks`, `stat:editor.encoding`, `stat:workbench.tasks`, `mod:src/editor/lsp/hint.ts`, `mod:src/platform/paths.ts`, `ipc:doc_set_encoding`, `ipc:lsp_completion`, `ipc:plugin_answer_prompt`, `ipc:plugin_market`, `ipc:recent_projects`, `ipc:run_start`, `ipc:settings_export`, `evt:ExcludedDirsChanged`, `evt:LanguageServer`, `unit:src/platform/paths.test.ts`
- **Q-FU2** Follow upstream 2: native tests that assert pre-merge behaviour (`docs/gpui-port/notes/reports/int_follow_upstream2_brief.md`): 18 rows. `act:vcs.log.dropCommit`, `act:vcs.log.editMessage`, `act:vcs.log.fixup`, `act:vcs.log.interactiveRebase`, `act:vcs.log.squashInto`, `act:vcs.log.undoCommit`, `unit:src/features/build-tools/ExecutePopup.test.tsx`, `unit:src/features/github/data/data.test.ts`, `unit:src/features/perf/__tests__/perf.test.ts`, `unit:src/features/plugins/indexedMarkers.test.ts`, `unit:src/features/plugins/markerIcons.test.ts`, `unit:src/features/settings/__tests__/inspections.test.ts`, `unit:src/features/templates/__tests__/live.test.ts`, `unit:src/features/vcs/__tests__/diffDialog.test.tsx`, `unit:src/features/vcs/__tests__/historyOps.test.ts`, `unit:src/features/vcs/__tests__/ignore.test.ts`, `unit:src/platform/keymaps/ideaParity.test.ts`, `unit:src/platform/remoteSettings.test.ts`
- **Q-FU4** Follow upstream 4: few-line action differences (`docs/gpui-port/notes/reports/int_follow_upstream4_brief.md`): 39 rows. `act:(dynamic) src/features/plugins/points.tsx:200`, `act:agents.forkReply`, `act:agents.forkTask`, `act:code.applyQuickFix`, `act:code.quickDefinition`, `act:db.execute`, `act:db.explainPlan`, `act:db.goToColumn`, `act:db.newDataSource`, `act:db.queryHistory`, `act:db.rename`, `act:db.revertChanges`, `act:db.submitChanges`, `act:edit.copy`, `act:edit.cut`, `act:edit.paste`, `act:edit.redo`, `act:file.openFile`, `act:file.openInNewWindow`, `act:http.openEndpoint`, `act:project.previewRestricted`, `act:project.revokeTrust`, `act:terminal.openAt`, `act:toolwindow.hideActive`, `act:vcs.fetch`, `act:vcs.ignoreLocal`, `act:vcs.ignoreShared`, `act:vcs.log.branches.update`, `act:vcs.log.changes.jumpToSource`, `act:vcs.log.changes.showDiff`, `act:vcs.merge`, `act:vcs.pull`, `act:vcs.rebase`, `act:vcs.resetHead`, `act:vcs.rollback`, `act:vcs.stash`, `act:vcs.unstash`, `act:vcs.update`, `ctx:vcsLogChangesFocus`
- **Q-RC** Remote folder chooser (Deployment, Run Targets) + Debug redaction of agents' environments (`docs/gpui-port/notes/reports/int_remote_chooser_brief.md`): 1 rows. `set:build.deployment`
- **Q-VIS2** Second visual slice (tool window header slot, Shift+Tab unindent, two unproven save cases) (`docs/gpui-port/notes/reports/int_visual2_brief.md`): 0 rows.
- **Q-AGL** Coding Agents list page (agents.list), draft ready (`docs/gpui-port/notes/reports/int_agents_list_brief.md`): 10 rows. `set:agents.list`, `ipc:agent_custom_import`, `ipc:agent_custom_remove`, `ipc:agent_custom_save`, `ipc:agent_registry_install`, `ipc:agent_registry_list`, `ipc:agent_registry_remove`, `ipc:agent_set_command`, `ipc:agent_set_env`, `unit:src/features/agents/agentRegistry.test.tsx`

## 2. The slices, in order

Order: first what is wrong, unsafe or losing data and is not in a queued brief (T1, by hand); then every other slice by rows it closes or moves per integration (rows ÷ parts, parts = ⌈lines to write ÷ 1,500⌉), never before a slice it depends on; an infrastructure slice (T2) takes the rank of the best slice that needs it. The queued briefs are assumed to land first, in the manager's order: follow upstream 3 (launched at 56b31cf21) → follow upstream 2 → follow upstream 4 → remote chooser + Debug redaction → Coding Agents list → second visual slice. New from Template landed while this plan was written (D149, 11014c6eb; inventory 1,188/3,782): what it left is the slice `F-NFT-REST`.

| #   | Slice                                                                                                                                                                                                                                                                                      | Tier | Rows (none / partial / model) | Spec cases to prove | Webview lines to read | Lines to write (Hypothesis) | Parts | Depends on                                                                                                                                                                                                                                                                     |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---- | ----------------------------- | ------------------- | --------------------- | --------------------------- | ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | `T1-AGENT-UNDO` An agent's workspace edit is recorded for Edit › Undo (dropped today)                                                                                                                                                                                                      | T1   | 2 (0 / 2 / 0)                 | 1                   | 307                   | 250                         | 1     | —                                                                                                                                                                                                                                                                              |
| 2   | `T1-RUN-GUARDS` Rerun stops live debug sessions; Debug on a running configuration asks; idle tab claimed                                                                                                                                                                                   | T1   | 5 (2 / 3 / 0)                 | 6                   | 3002                  | 1128                        | 1     | Q-FU3                                                                                                                                                                                                                                                                          |
| 3   | `T1-TEMPLATES-UP` Templates follow upstream: Enter in a completion's snippet, Java .var through Introduce Variable, Zig postfix/surrounders                                                                                                                                                | T1   | 4 (0 / 4 / 0)                 | 6                   | 1938                  | 656                         | 1     | Q-NFT                                                                                                                                                                                                                                                                          |
| 4   | `T1-PLUGIN-VISUALS` Plugin decorations, line markers and colours follow upstream; the line-marker gutter is reached by the app                                                                                                                                                             | T1   | 14 (9 / 4 / 1)                | 20                  | 1833                  | 1216                        | 1     | Q-FU2                                                                                                                                                                                                                                                                          |
| 5   | `T1-DEPLOY-FTP` Deployment servers page: FTP/FTPS fields, FTP password staging, server groups (a native removal leaves dangling group members)                                                                                                                                             | T1   | 6 (3 / 3 / 0)                 | 12                  | 2000                  | 818                         | 1     | Q-RC                                                                                                                                                                                                                                                                           |
| 6   | `T1-SAFETY-CHECKS` Owed safety checks with no row: Restricted Mode writes, Restart keeps the isolated home, tests off the user's global git ignore and mise config                                                                                                                         | T1   | 0 (0 / 0 / 0)                 | 0                   | 2002                  | 300                         | 1     | —                                                                                                                                                                                                                                                                              |
| 7   | `F-SEARCH-B` Search: Search Everywhere, recent files and locations, file structure, bookmarks, dialogs                                                                                                                                                                                     | T3   | 25 (3 / 21 / 1)               | 25                  | 980                   | 1140                        | 1     | —                                                                                                                                                                                                                                                                              |
| 8   | `F-SET-JDK-LANGS` JDK pages (add from disk, jdtls JDK, Download JDK as a listed-not-run flow), Java formatter, Go build tags, CMake, Zig, Node/TS/Ruby/Rust/Go pages, module overrides, project dependencies, Project Structure                                                            | T3   | 23 (11 / 12 / 0)              | 30                  | 1390                  | 1390                        | 1     | —                                                                                                                                                                                                                                                                              |
| 9   | `F-SET-UPSTREAM-TOGGLES-B` Settings pages gain upstream's new settings (system notifications, debugger JS / live watches, files in associated apps, terminal highlight, project-view branch, dates): Keymap page, assign shortcut, system hotkeys, page registry, layout e2e specs         | T3   | 22 (14 / 7 / 1)               | 30                  | 1325                  | 990                         | 1     | —                                                                                                                                                                                                                                                                              |
| 10  | `I-TW-CONTENT-TABS` Content tabs in a tool window (closable, pinnable, tab menu) on the shared header                                                                                                                                                                                      | T2   | 9 (7 / 2 / 0)                 | 13                  | 1690                  | 1236                        | 1     | Q-VIS2                                                                                                                                                                                                                                                                         |
| 11  | `F-HIERARCHY-USAGES` Hierarchy tabs (pin, root detail, prefetched levels), Find Usages popup and results, problems, server status, ESLint, type hierarchy prepare                                                                                                                          | T3   | 20 (5 / 15 / 0)               | 22                  | 6796                  | 1497                        | 1     | I-TW-CONTENT-TABS, Q-FU3                                                                                                                                                                                                                                                       |
| 12  | `F-PLAT-PROJECT-A` Platform project models: project, roots, presentation, trust model, remote, virtual docs, class files, file manager, file ops                                                                                                                                           | T3   | 19 (9 / 10 / 0)               | 15                  | 1957                  | 1466                        | 1     | —                                                                                                                                                                                                                                                                              |
| 13  | `F-SET-UPSTREAM-TOGGLES-A` Settings pages gain upstream's new settings (system notifications, debugger JS / live watches, files in associated apps, terminal highlight, project-view branch, dates): the pages' new settings (one line each) and the dialog spec                           | T3   | 18 (3 / 15 / 0)               | 23                  | 1102                  | 1074                        | 1     | —                                                                                                                                                                                                                                                                              |
| 14  | `I-UNTITLED` Untitled buffers (File › New Untitled; VS Code presets' Cmd/Ctrl+N does nothing natively today)                                                                                                                                                                               | T2   | 6 (4 / 2 / 0)                 | 7                   | 3541                  | 900                         | 1     | —                                                                                                                                                                                                                                                                              |
| 15  | `F-SMALL-FEATURES-A` Small features: build (compile file, module), web preview, project trust, welcome                                                                                                                                                                                     | T3   | 35 (19 / 16 / 0)              | 9                   | 943                   | 1782                        | 2     | I-UNTITLED                                                                                                                                                                                                                                                                     |
| 16  | `F-PLAT-MODELS-C-B` Platform models III: error log, platform info, proxy, workflow, client and editor settings, small platform modules                                                                                                                                                     | T3   | 33 (23 / 10 / 0)              | 13                  | 3456                  | 2667                        | 2     | Q-FU3                                                                                                                                                                                                                                                                          |
| 17  | `F-SMALL-FEATURES-C` Small features: TODO, Rails, Thymeleaf, remote development and ports, Logcat (device stand-in), window zoom, style specs                                                                                                                                              | T3   | 33 (22 / 5 / 6)               | 34                  | 2455                  | 2328                        | 2     | —                                                                                                                                                                                                                                                                              |
| 18  | `F-VCS-REPO-DIALOGS-A` Git dialogs pick the repository (merge, pull, rebase, tag, push), operation repository, multi-root: dialogs pick the repository: merge, pull, rebase, tag, operation repository, multi-root, branch ops                                                             | T3   | 16 (7 / 9 / 0)                | 32                  | 2619                  | 1102                        | 1     | Q-FU4                                                                                                                                                                                                                                                                          |
| 19  | `F-DBG-CONSOLE-WATCHES-B` Debug console (links, find, history, input to the program), expression completion, live watches, watch storage, event breakpoints, class imports, startup breakpoints: watches, values, evaluation, breakpoints (event, startup), class imports, execution state | T3   | 16 (8 / 8 / 0)                | 49                  | 1771                  | 1288                        | 1     | —                                                                                                                                                                                                                                                                              |
| 20  | `F-WB-SHELL-A` Workbench shell: Workbench, project window and colour, quick switch, decorations, providers, file editors, endpoint providers                                                                                                                                               | T3   | 16 (7 / 7 / 2)                | 22                  | 1481                  | 954                         | 1     | —                                                                                                                                                                                                                                                                              |
| 21  | `F-APP-SHELL-C` App shell: shell, menu bridge, CLI, log, about, caches, open paths, export/import settings                                                                                                                                                                                 | T3   | 31 (16 / 14 / 1)              | 13                  | 993                   | 1871                        | 2     | Q-FU3                                                                                                                                                                                                                                                                          |
| 22  | `F-LSP-REFACTOR-B` Refactorings through the server: Java move, introduce (variable, constant, field, parameter, method), server introduce, cancellation                                                                                                                                    | T3   | 15 (8 / 7 / 0)                | 20                  | 2063                  | 1296                        | 1     | —                                                                                                                                                                                                                                                                              |
| 23  | `F-LSP-REFACTOR-C` Refactorings through the server: Refactor This, safe delete, run kind, catalog and the rest                                                                                                                                                                             | T3   | 15 (7 / 8 / 0)                | 4                   | 2713                  | 1338                        | 1     | —                                                                                                                                                                                                                                                                              |
| 24  | `F-VIEWERS` Viewers: Markdown preview search highlight, Mermaid full screen/PNG, local SVG, zoom, binary and image editors, associated applications                                                                                                                                        | T3   | 15 (6 / 9 / 0)                | 35                  | 6092                  | 1382                        | 1     | —                                                                                                                                                                                                                                                                              |
| 25  | `F-GH-REVIEW-B` GitHub pull requests: timeline, pull request tool window, create, update/delete branch, the rest                                                                                                                                                                           | T3   | 29 (19 / 9 / 1)               | 12                  | 788                   | 2047                        | 2     | —                                                                                                                                                                                                                                                                              |
| 26  | `I-UNDO-PER-DOC` One undo history per document shared by its views (tab, code previews, Find previews)                                                                                                                                                                                     | T2   | 1 (0 / 1 / 0)                 | 4                   | 1152                  | 393                         | 1     | —                                                                                                                                                                                                                                                                              |
| 27  | `F-SEARCH-A` Search: Find in Files and Replace (scope editor, resizing, previews), Find tool window                                                                                                                                                                                        | T3   | 14 (4 / 10 / 0)               | 14                  | 1282                  | 522                         | 1     | I-UNDO-PER-DOC                                                                                                                                                                                                                                                                 |
| 28  | `F-RUN-SERVICES` Services window: configuration folders, running badge, group actions on folders (Run/Restart/Stop All, Open Browser)                                                                                                                                                      | T3   | 13 (3 / 9 / 1)                | 19                  | 2256                  | 1157                        | 1     | Q-FU2                                                                                                                                                                                                                                                                          |
| 29  | `F-SET-FILE-TYPES` File types (override file type, revert), file colours, scopes page, caches local data folder, MCP server page, plugin formatters fold                                                                                                                                   | T3   | 13 (9 / 4 / 0)                | 4                   | 290                   | 907                         | 1     | —                                                                                                                                                                                                                                                                              |
| 30  | `F-RUN-TYPES-OTHER-A` Run types II: executors, targets, remote targets, sessions, runtimes, the type registry, navigation, stack traces                                                                                                                                                    | T3   | 25 (10 / 13 / 2)              | 74                  | 4440                  | 2714                        | 2     | —                                                                                                                                                                                                                                                                              |
| 31  | `F-PL-NAV-EVENTS` Plugin navigation (index, markers, goto, files), workbench events to plugins (DocumentChanged, WebsocketReady, ProblemsChanged), endpoints, workspace contains, command backend, plugin text                                                                             | T3   | 25 (17 / 7 / 1)               | 14                  | 2719                  | 1882                        | 2     | T1-PLUGIN-VISUALS                                                                                                                                                                                                                                                              |
| 32  | `F-VCS-LOG` Git Log: ref labels, details batch, panes, loading, options, graph, branch memory, search, filter presets                                                                                                                                                                      | T3   | 36 (10 / 24 / 2)              | 47                  | 7299                  | 3172                        | 3     | Q-FU2, Q-FU4                                                                                                                                                                                                                                                                   |
| 33  | `F-EDITOR-TABS-SESSIONS-A` Editor tabs and sessions: tabs: pinned row, kept tabs, title revision, detach, layout actions, group panels                                                                                                                                                     | T3   | 12 (4 / 7 / 1)                | 14                  | 6603                  | 694                         | 1     | —                                                                                                                                                                                                                                                                              |
| 34  | `F-HTTP` HTTP Client: environment files (add public/private), Copy as cURL and paste conversion, file and variable navigation, headers, response find, endpoints tab, Spring                                                                                                               | T3   | 24 (16 / 8 / 0)               | 13                  | 4661                  | 1567                        | 2     | Q-FU4, Q-VIS2                                                                                                                                                                                                                                                                  |
| 35  | `I-ACTIONS-ENABLED` Actions: `enabled(ctx, arg)` predicate, `indicatesRunning`, batched registration                                                                                                                                                                                       | T2   | 4 (0 / 4 / 0)                 | 4                   | 380                   | 253                         | 1     | Q-FU4                                                                                                                                                                                                                                                                          |
| 36  | `F-PLAT-MODELS-A` Platform models I: lossless JSON, scopes, double click, pointer focus, navigation modifier, keymap mouse shortcuts, IDEA keymap import                                                                                                                                   | T3   | 23 (16 / 7 / 0)               | 57                  | 3548                  | 2627                        | 2     | Q-FU4                                                                                                                                                                                                                                                                          |
| 37  | `F-LSP-INLINE-B` Inline completion, inlay hints and requests, code lens (server and author), interactive commands, format errors, spelling: code lens (server, author), interactive commands, format, spelling                                                                             | T3   | 23 (14 / 8 / 1)               | 39                  | 2685                  | 2038                        | 2     | —                                                                                                                                                                                                                                                                              |
| 38  | `F-VCS-HISTORY` File history, revisions, annotate a revision, blame, Show Diff for Lines, local history, recent changes                                                                                                                                                                    | T3   | 23 (13 / 8 / 2)               | 33                  | 3634                  | 2305                        | 2     | I-ACTIONS-ENABLED                                                                                                                                                                                                                                                              |
| 39  | `F-LSP-POPUPS-C` Completion, hover, docs, signature, intentions, quick fixes, type info, hint, Popups, auto import and imports on paste, in-place rename, outside-project files: intentions, quick fixes, rename in place, imports, outside project, availability, diagnostics             | T3   | 34 (21 / 11 / 2)              | 28                  | 5405                  | 3348                        | 3     | Q-FU3, Q-FU4                                                                                                                                                                                                                                                                   |
| 40  | `F-RUN-TESTS` Test runner: tree view, rerun failed, expand/collapse, comparison, plugin/dotnet/Zig/Ruby tests, test reports                                                                                                                                                                | T3   | 22 (15 / 3 / 4)               | 36                  | 6373                  | 2133                        | 2     | —                                                                                                                                                                                                                                                                              |
| 41  | `F-PL-DETAILS` Plugins page: Requires / Required by / Extension points / net hosts sections, revoke a net host, ask before disabling a needed plugin, install from disk, IntelliJ themes                                                                                                   | T3   | 22 (12 / 10 / 0)              | 72                  | 5170                  | 2854                        | 2     | Q-FU3                                                                                                                                                                                                                                                                          |
| 42  | `F-RUN-TYPES-OTHER-B` Run types II: language types: .NET, Go, PHP Composer, npm, Bun, Ruby/Rake, Unity Editor, launch settings                                                                                                                                                             | T3   | 18 (15 / 2 / 1)               | 10                  | 892                   | 1337                        | 1     | —                                                                                                                                                                                                                                                                              |
| 43  | `F-TERMINAL` Terminal: find bar, move to editor, CLI sessions and kinds, links and highlights, plugin sessions, close last tab, embedded fit, IME, paste, image store                                                                                                                      | T3   | 32 (20 / 10 / 2)              | 90                  | 12022                 | 3721                        | 3     | —                                                                                                                                                                                                                                                                              |
| 44  | `F-EDITOR-TABS-SESSIONS-B` Editor tabs and sessions: sessions: busy and session close, input close, file-open order, light edit, navigation history                                                                                                                                        | T3   | 21 (11 / 9 / 1)               | 53                  | 4978                  | 2427                        | 2     | —                                                                                                                                                                                                                                                                              |
| 45  | `F-EDITOR-DOCS-SYNC-B` Documents: library sources, file attributes, encoding and BOM, banners, status items, copy reference                                                                                                                                                                | T3   | 21 (14 / 7 / 0)               | 27                  | 1960                  | 1848                        | 2     | Q-FU3                                                                                                                                                                                                                                                                          |
| 46  | `F-AG-COMPOSER-TASKS-C` Agent composer and tasks: changes panel, home editor, AgentsView, unread, working status, policy, status item                                                                                                                                                      | T3   | 21 (13 / 8 / 0)               | 54                  | 3511                  | 2232                        | 2     | Q-FU2                                                                                                                                                                                                                                                                          |
| 47  | `F-DB-CONSOLES` Several query consoles per data source, New Query Console, Jump to Console list, SQL script, MyBatis, SQL resolution scopes, Snowflake commands and highlighting                                                                                                           | T3   | 21 (14 / 7 / 0)               | 30                  | 6107                  | 2269                        | 2     | —                                                                                                                                                                                                                                                                              |
| 48  | `F-DB-TREE-SOURCES-A` Database tree and sources: introspection, objects, the tree, table editor and table dialogs                                                                                                                                                                          | T3   | 21 (11 / 10 / 0)              | 29                  | 2741                  | 1922                        | 2     | —                                                                                                                                                                                                                                                                              |
| 49  | `F-WB-SHELL-B` Workbench shell: the workbench's extension points and remaining modules (each closes in a few lines once its user exists)                                                                                                                                                   | T3   | 21 (13 / 8 / 0)               | 0                   | 3702                  | 2315                        | 2     | —                                                                                                                                                                                                                                                                              |
| 50  | `F-BT-SBT-ANALYZER-B` sbt tool window, Maven/Gradle Dependency Analyzer, Add/Unlink Maven projects, generate sources, skip tests, auto-reload, first import, profile reload: sbt tool window, auto-reload, first import, profile reload, generate sources, skip tests                      | T3   | 17 (14 / 3 / 0)               | 13                  | 1936                  | 1437                        | 1     | —                                                                                                                                                                                                                                                                              |
| 51  | `F-VCS-REPO-DIALOGS-B` Git dialogs pick the repository (merge, pull, rebase, tag, push), operation repository, multi-root: push: changes, single push, targets, target branch input, rejected push, remote results, update project                                                         | T3   | 10 (5 / 4 / 1)                | 39                  | 1410                  | 877                         | 1     | Q-FU4                                                                                                                                                                                                                                                                          |
| 52  | `F-PLAT-MODELS-C-A` Platform models III: i18n, text catalog, date formats, release notes, splash, boot                                                                                                                                                                                     | T3   | 10 (6 / 4 / 0)                | 38                  | 2300                  | 1091                        | 1     | Q-FU3                                                                                                                                                                                                                                                                          |
| 53  | `F-PROJECT-VIEW` Project view: rename in place, Show Members, root paths, open in right split, Scratches and Unity panes, extra roots, Maven roots, module groups, clipboard of files, context menu                                                                                        | T3   | 40 (19 / 20 / 1)              | 88                  | 15263                 | 5007                        | 4     | I-ACTIONS-ENABLED                                                                                                                                                                                                                                                              |
| 54  | `F-AG-SESSION-FEATURES` Agent session: selection toolbar (Quote, side conversation), find in conversation, focus input, retry, permission forms and command titles, queued messages, background stop                                                                                       | T3   | 29 (20 / 9 / 0)               | 94                  | 11914                 | 4334                        | 3     | —                                                                                                                                                                                                                                                                              |
| 55  | `F-DBG-ATTACH` Attach to Process, Unity attach, remote debug types (Go, Node, PHP, Python, JVM, Godot), adapter choice, missing adapter                                                                                                                                                    | T3   | 16 (13 / 3 / 0)               | 23                  | 1256                  | 1481                        | 1     | Q-FU2                                                                                                                                                                                                                                                                          |
| 56  | `F-LSP-NAV` Language navigation: chooser, shared navigation, implementations, imported declarations, pending navigation, mouse navigation, document links, file references                                                                                                                 | T3   | 19 (13 / 6 / 0)               | 51                  | 3644                  | 2145                        | 2     | Q-FU3                                                                                                                                                                                                                                                                          |
| 57  | `F-SVC-DOCKER-B` Services: Compose (up, gutter, file override, auto-detect), dashboard, connection dialog, the rest                                                                                                                                                                        | T3   | 19 (13 / 6 / 0)               | 26                  | 1914                  | 1571                        | 2     | —                                                                                                                                                                                                                                                                              |
| 58  | `F-VCS-DIFF-FIND` Diff viewer: find bar, chains, providers, external diff tool, blank diff, save from the diff window                                                                                                                                                                      | T3   | 19 (13 / 6 / 0)               | 49                  | 2357                  | 1819                        | 2     | I-ACTIONS-ENABLED                                                                                                                                                                                                                                                              |
| 59  | `F-WB-TOOLWINDOWS` Tool window store and layouts: maximize, view modes, layouts (delete, rename, restore), focus, narrow window, Zen, distraction-free                                                                                                                                     | T3   | 19 (7 / 12 / 0)               | 45                  | 4513                  | 2242                        | 2     | I-TW-CONTENT-TABS                                                                                                                                                                                                                                                              |
| 60  | `F-NFT-REST` What New from Template (landed in D149, 11014c6eb) left partial: the remaining cases of its specs and rows                                                                                                                                                                    | T3   | 9 (1 / 8 / 0)                 | 21                  | 2660                  | 785                         | 1     | —                                                                                                                                                                                                                                                                              |
| 61  | `F-VCS-MERGE` Merge editor, conflict markers in the editor, Git merge diff                                                                                                                                                                                                                 | T3   | 9 (5 / 2 / 2)                 | 13                  | 1547                  | 902                         | 1     | —                                                                                                                                                                                                                                                                              |
| 62  | `F-LSP-POPUPS-B` Completion, hover, docs, signature, intentions, quick fixes, type info, hint, Popups, auto import and imports on paste, in-place rename, outside-project files: hover, docs, signature, Popups, Markdown, hint, type info                                                 | T3   | 18 (9 / 9 / 0)                | 14                  | 2823                  | 1788                        | 2     | Q-FU3, Q-FU4                                                                                                                                                                                                                                                                   |
| 63  | `F-WB-STATUS-TASKS` Status bar: unavailable widgets, info item, tasks, background processes (stop), running processes provider                                                                                                                                                             | T3   | 9 (3 / 5 / 1)                 | 6                   | 949                   | 561                         | 1     | Q-FU3                                                                                                                                                                                                                                                                          |
| 64  | `F-APP-WORKSPACE` Workspaces: create, manage, open, attach/detach projects, discover projects, workspace dialog, project types                                                                                                                                                             | T3   | 15 (14 / 1 / 0)               | 6                   | 1251                  | 1477                        | 1     | —                                                                                                                                                                                                                                                                              |
| 65  | `F-WB-MENUS-TITLE` Main menu (interaction, toolbar style row), title bar (New Project…, Create Repository fallback), native title bar, window controls and drag, input dialogs, main toolbar dialog                                                                                        | T3   | 30 (25 / 5 / 0)               | 47                  | 2970                  | 2546                        | 2     | I-ACTIONS-ENABLED                                                                                                                                                                                                                                                              |
| 66  | `F-PERF-MCP-SSH` Perf (memory indicator, overlay, clear caches, end process for own processes only), MCP requests, SSH sessions and askpass                                                                                                                                                | T3   | 29 (27 / 2 / 0)               | 14                  | 1857                  | 2260                        | 2     | Q-FU2                                                                                                                                                                                                                                                                          |
| 67  | `F-VCS-COMMIT-DIALOG` Modal Commit dialog, commit from context selection, commit checks, plugin commit checks                                                                                                                                                                              | T3   | 17 (11 / 6 / 0)               | 23                  | 1787                  | 1651                        | 2     | —                                                                                                                                                                                                                                                                              |
| 68  | `F-LSP-INLINE-A` Inline completion, inlay hints and requests, code lens (server and author), interactive commands, format errors, spelling: inline completion and inlay hints                                                                                                              | T3   | 14 (12 / 1 / 1)               | 15                  | 1179                  | 1431                        | 1     | —                                                                                                                                                                                                                                                                              |
| 69  | `F-GH-REVIEW-A` GitHub pull requests: review mode and review diff comments                                                                                                                                                                                                                 | T3   | 14 (12 / 2 / 0)               | 2                   | 1473                  | 1406                        | 1     | —                                                                                                                                                                                                                                                                              |
| 70  | `F-VCS-BRANCHES` Branches popup and widget (expansion, resize, host), branch widget extensions                                                                                                                                                                                             | T3   | 8 (3 / 5 / 0)                 | 5                   | 2241                  | 841                         | 1     | —                                                                                                                                                                                                                                                                              |
| 71  | `F-EDITOR-CORE-EDITING` Editor editing: smart keys, editing, clipboard, string clipboard, doc comments, indent guides, lines, IDEA parity, editing extras, paste history                                                                                                                   | T3   | 24 (3 / 21 / 0)               | 56                  | 21848                 | 3465                        | 3     | Q-VIS2                                                                                                                                                                                                                                                                         |
| 72  | `F-RUN-CHOOSER` Run configuration chooser: All Configurations fold, Current File after, row ⋮ menu (Run, Debug, Edit, Pin, Delete), Unity defaults                                                                                                                                         | T3   | 8 (4 / 4 / 0)                 | 22                  | 1683                  | 962                         | 1     | Q-FU2                                                                                                                                                                                                                                                                          |
| 73  | `F-NOTEBOOK` Notebook: cell docs, output view, security, session (kernel stand-in)                                                                                                                                                                                                         | T3   | 8 (2 / 6 / 0)                 | 4                   | 3688                  | 714                         | 1     | —                                                                                                                                                                                                                                                                              |
| 74  | `I-WINDOW-GROUPS` Window tab groups (merge, detach, reorder, select, close together, restore at start)                                                                                                                                                                                     | T2   | 26 (24 / 1 / 1)               | 4                   | 260                   | 2400                        | 2     | —                                                                                                                                                                                                                                                                              |
| 75  | `F-PLAT-PROJECT-B` Platform project models: JDK, .NET, build model and recovery, page swipe, aux window, clipboard, IME, native drop                                                                                                                                                       | T3   | 26 (19 / 6 / 1)               | 30                  | 2326                  | 1999                        | 2     | —                                                                                                                                                                                                                                                                              |
| 76  | `F-GRAPHQL` GraphQL: configurations, introspection, run query, environment, tool window                                                                                                                                                                                                    | T3   | 13 (11 / 1 / 1)               | 3                   | 3314                  | 1425                        | 1     | —                                                                                                                                                                                                                                                                              |
| 77  | `F-RUN-IMPORT` Import run configurations (JetBrains, VS Code launch.json), run templates, context run producers                                                                                                                                                                            | T3   | 15 (8 / 7 / 0)                | 23                  | 5841                  | 2437                        | 2     | —                                                                                                                                                                                                                                                                              |
| 78  | `F-PL-VIEWS-B` Plugin views: web views (pty, files), HTML frame, plugin web shell module                                                                                                                                                                                                   | T3   | 12 (9 / 1 / 2)                | 19                  | 1023                  | 822                         | 1     | —                                                                                                                                                                                                                                                                              |
| 79  | `F-APP-SHELL-B` App shell: JetBrains settings import and updates                                                                                                                                                                                                                           | T3   | 12 (10 / 2 / 0)               | 19                  | 1760                  | 1184                        | 1     | Q-FU3                                                                                                                                                                                                                                                                          |
| 80  | `F-MACROS-RUNANY-SCRATCH` Macros (record, play, edit), Run Anything, scratch files                                                                                                                                                                                                         | T3   | 24 (22 / 0 / 2)               | 14                  | 2127                  | 2227                        | 2     | —                                                                                                                                                                                                                                                                              |
| 81  | `F-PLAT-MODELS-B-A` Platform models II: fonts, themes, colour schemes, ICLS, CSS colours (models)                                                                                                                                                                                          | T3   | 14 (3 / 11 / 0)               | 29                  | 3382                  | 1587                        | 2     | —                                                                                                                                                                                                                                                                              |
| 82  | `F-LSP-POPUPS-A` Completion, hover, docs, signature, intentions, quick fixes, type info, hint, Popups, auto import and imports on paste, in-place rename, outside-project files: completion (55 cases) and its items                                                                       | T3   | 7 (4 / 3 / 0)                 | 20                  | 2348                  | 1111                        | 1     | Q-FU3, Q-FU4                                                                                                                                                                                                                                                                   |
| 83  | `F-EDITOR-DOCS-SYNC-A` Documents: disk sync, save on blur, custom and virtual documents                                                                                                                                                                                                    | T3   | 14 (8 / 6 / 0)                | 37                  | 5499                  | 2259                        | 2     | Q-FU3                                                                                                                                                                                                                                                                          |
| 84  | `F-AG-COMPOSER-TASKS-B` Agent composer and tasks: tasks: groups, pin, close, organize, project, dock, import sessions                                                                                                                                                                      | T3   | 23 (17 / 6 / 0)               | 29                  | 1004                  | 2062                        | 2     | Q-FU2                                                                                                                                                                                                                                                                          |
| 85  | `F-COVERAGE-PROFILER` Coverage (show, hide, import, manage reports, gutter) and profiler (CPU profiles, executors, live objects)                                                                                                                                                           | T3   | 34 (32 / 1 / 1)               | 25                  | 2932                  | 3179                        | 3     | —                                                                                                                                                                                                                                                                              |
| 86  | `F-VCS-REMOTE-LINKS` Open on remote / Copy remote link, file web URLs, clone, create repository, worktrees                                                                                                                                                                                 | T3   | 11 (10 / 1 / 0)               | 14                  | 1576                  | 1413                        | 1     | —                                                                                                                                                                                                                                                                              |
| 87  | `F-LSP-REFACTOR-A` Refactorings through the server: dialogs: Extract Interface, Change Signature, hierarchy (Pull Up / Push Down)                                                                                                                                                          | T3   | 22 (22 / 0 / 0)               | 19                  | 1599                  | 2170                        | 2     | —                                                                                                                                                                                                                                                                              |
| 88  | `F-RUN-TYPES-OTHER-C` Run types II: plugin attach and start, agent runs, container debug, plugin programs                                                                                                                                                                                  | T3   | 11 (10 / 0 / 1)               | 7                   | 322                   | 787                         | 1     | —                                                                                                                                                                                                                                                                              |
| 89  | `F-DBG-JS-BROWSER` JavaScript browser debugging: Debug in Browser, browser sessions, Network tab, response overrides, page profile, JS custom breakpoints                                                                                                                                  | T3   | 22 (21 / 1 / 0)               | 65                  | 2635                  | 2281                        | 2     | —                                                                                                                                                                                                                                                                              |
| 90  | `F-DBG-CONSOLE-WATCHES-A` Debug console (links, find, history, input to the program), expression completion, live watches, watch storage, event breakpoints, class imports, startup breakpoints: debug console: links, find, history, input, completion, process output, service console   | T3   | 11 (11 / 0 / 0)               | 23                  | 543                   | 834                         | 1     | —                                                                                                                                                                                                                                                                              |
| 91  | `F-ANALYSIS` Code analysis: Inspect Code / File, inheritance inspections (Python), locate duplicates, cleanup, structural search and replace                                                                                                                                               | T3   | 22 (20 / 0 / 2)               | 29                  | 4081                  | 2415                        | 2     | —                                                                                                                                                                                                                                                                              |
| 92  | `F-SMALL-FEATURES-B` Small features: libraries, colour preview, what's new, Cucumber, dependencies                                                                                                                                                                                         | T3   | 22 (19 / 2 / 1)               | 37                  | 1739                  | 1761                        | 2     | —                                                                                                                                                                                                                                                                              |
| 93  | `F-UI-PRIMITIVES` Design-system primitives: Tree (+reorder), Tabs, Select, Dialog, List, Popup, FloatingToolbar, Toolbar, Splitter, Scrollbar, Mnemonic, Button, LogPane, SourcePreview, focus scope, selection boundary, drag cursor, icons                                               | T3   | 76 (59 / 14 / 3)              | 199                 | 21740                 | 9694                        | 7     | —                                                                                                                                                                                                                                                                              |
| 94  | `F-DB-TREE-SOURCES-B` Database tree and sources: sources (dialog, connect, SSH tunnel secret), export, schema compare, import                                                                                                                                                              | T3   | 13 (9 / 4 / 0)                | 25                  | 2374                  | 1683                        | 2     | —                                                                                                                                                                                                                                                                              |
| 95  | `F-APP-SHELL-A` App shell: close guard and windows                                                                                                                                                                                                                                         | T3   | 13 (4 / 9 / 0)                | 46                  | 2277                  | 1575                        | 2     | I-UNTITLED, Q-FU3                                                                                                                                                                                                                                                              |
| 96  | `F-EDITOR-EMBEDDED-LANGS` Embedded languages in the editor: Angular, Astro, Django, JSP, ERB, Vue, Thymeleaf, Helm, go.mod, Gherkin, shell interpolation, linked tags                                                                                                                      | T3   | 32 (29 / 3 / 0)               | 155                 | 5536                  | 3895                        | 3     | —                                                                                                                                                                                                                                                                              |
| 97  | `F-RUN-TYPES-JVM` Run types I: Java / JVM module, Gradle run, Tomcat, JAR application, Java debug, remote JVM, application build/publish/update                                                                                                                                            | T3   | 21 (16 / 5 / 0)               | 43                  | 3134                  | 2076                        | 2     | —                                                                                                                                                                                                                                                                              |
| 98  | `F-SVC-DOCKER-A` Services: containers and images: actions, logs, inspect, processes, layers                                                                                                                                                                                                | T3   | 21 (21 / 0 / 0)               | 0                   | 6318                  | 2715                        | 2     | —                                                                                                                                                                                                                                                                              |
| 99  | `F-AG-COMPOSER-TASKS-A` Agent composer and tasks: composer: folders, submission, commands, draft and click e2e                                                                                                                                                                             | T3   | 10 (8 / 2 / 0)                | 49                  | 1690                  | 1152                        | 1     | Q-FU2                                                                                                                                                                                                                                                                          |
| 100 | `F-PL-VIEWS-A` Plugin views: content editor, editor panels, view windows, logs, projection, project UI                                                                                                                                                                                     | T3   | 20 (16 / 4 / 0)               | 64                  | 2226                  | 2172                        | 2     | —                                                                                                                                                                                                                                                                              |
| 101 | `F-PL-VIEWS-C` Plugin views: service pane and provider, text editor wrappers, file editor providers                                                                                                                                                                                        | T3   | 10 (10 / 0 / 0)               | 15                  | 365                   | 810                         | 1     | —                                                                                                                                                                                                                                                                              |
| 102 | `F-WIT` Plugin API interfaces (wit/): one test per interface through a test plugin, after the feature slice that surfaces it                                                                                                                                                               | T3   | 46 (33 / 13 / 0)              | 0                   | 0                     | 2300                        | 2     | F-DB-TREE-SOURCES-A, F-DB-TREE-SOURCES-B, F-PL-NAV-EVENTS, F-PL-VIEWS-A, F-PL-VIEWS-B, F-PL-VIEWS-C, F-VCS-COMMIT-DIALOG                                                                                                                                                       |
| 103 | `F-BT-POM` POM completion and navigation, version catalog, published catalogs                                                                                                                                                                                                              | T3   | 10 (10 / 0 / 0)               | 15                  | 821                   | 832                         | 1     | —                                                                                                                                                                                                                                                                              |
| 104 | `F-TEMPLATES-REST` Templates: Emmet expansion (Tab, Django rule M8), templates in the completion list, Expressions chooser, plugin snippets, the feature row                                                                                                                               | T3   | 6 (2 / 4 / 0)                 | 4                   | 1049                  | 460                         | 1     | Q-NFT, T1-TEMPLATES-UP                                                                                                                                                                                                                                                         |
| 105 | `F-AG-MODELS` Agents' model catalog: ModelCatalog event, Refresh Models, cost and usage badges, Reset, effort picker cycling, pinned models                                                                                                                                                | T3   | 11 (7 / 4 / 0)                | 30                  | 3462                  | 1733                        | 2     | Q-AGL                                                                                                                                                                                                                                                                          |
| 106 | `I-SYSNOTIFY` System notifications while the app is in the background (setting, chime rule)                                                                                                                                                                                                | T2   | 9 (5 / 4 / 0)                 | 4                   | 656                   | 631                         | 1     | —                                                                                                                                                                                                                                                                              |
| 107 | `F-PLAT-MODELS-B-B` Platform models II: Appearance settings: colour scheme page, theme preview, icon themes, contrast, background, ligatures, e2e                                                                                                                                          | T3   | 18 (14 / 2 / 2)               | 47                  | 1261                  | 1566                        | 2     | —                                                                                                                                                                                                                                                                              |
| 108 | `F-SET-PYTHON-PHP-A` Python interpreters (status, add interpreter, container interpreter, docstrings) and PHP language level: Python interpreters                                                                                                                                          | T3   | 18 (15 / 3 / 0)               | 46                  | 1002                  | 1878                        | 2     | —                                                                                                                                                                                                                                                                              |
| 109 | `F-DB-GRID-A` Database grid: JSON value window and cell editors                                                                                                                                                                                                                            | T3   | 9 (9 / 0 / 0)                 | 21                  | 534                   | 991                         | 1     | I-ACTIONS-ENABLED                                                                                                                                                                                                                                                              |
| 110 | `F-VCS-CHANGES` Local Changes view: grouping by repository, directories, copy, ignore submenu, drops onto change lists, test-file colour                                                                                                                                                   | T3   | 17 (13 / 3 / 1)               | 64                  | 4395                  | 2196                        | 2     | Q-FU4                                                                                                                                                                                                                                                                          |
| 111 | `F-RUN-CONSOLE` Run console: colouring by level (console.logColors), copy, selection, find bar, output sample, terminal console                                                                                                                                                            | T3   | 17 (12 / 5 / 0)               | 46                  | 2459                  | 1536                        | 2     | —                                                                                                                                                                                                                                                                              |
| 112 | `F-DBG-HEAP-MEMORY` Heap snapshots and JVM memory view: capture, open, save, compare, dominators, instances, referrers                                                                                                                                                                     | T3   | 34 (34 / 0 / 0)               | 183                 | 5462                  | 5461                        | 4     | —                                                                                                                                                                                                                                                                              |
| 113 | `F-DBG-TAIL` Debugger: actions spec, launch, Java, hotswap, tool window, the feature row                                                                                                                                                                                                   | T3   | 13 (3 / 10 / 0)               | 34                  | 4132                  | 1647                        | 2     | F-DBG-ATTACH, F-DBG-CONSOLE-WATCHES-A, F-DBG-CONSOLE-WATCHES-B, F-DBG-HEAP-MEMORY, F-DBG-JS-BROWSER                                                                                                                                                                            |
| 114 | `F-DB-GRID-B` Database grid: Grid spec: booleans, enums, NULL / DEFAULT, copy, filter, rows                                                                                                                                                                                                | T3   | 17 (16 / 1 / 0)               | 30                  | 1867                  | 2589                        | 2     | I-ACTIONS-ENABLED                                                                                                                                                                                                                                                              |
| 115 | `I-STAGE-HUNKS` Stage hunks from the diff toolbar (integrator-direct, per the backlog)                                                                                                                                                                                                     | T2   | 1 (1 / 0 / 0)                 | 0                   | 154                   | 200                         | 1     | —                                                                                                                                                                                                                                                                              |
| 116 | `F-NAVIGATION` Navigation: related symbol, line markers (33 cases), Python hierarchy, header source, implementation gutter, inheritance graph, goto test, super method, structure                                                                                                          | T3   | 25 (19 / 4 / 2)               | 49                  | 6396                  | 3087                        | 3     | —                                                                                                                                                                                                                                                                              |
| 117 | `F-VCS-DIFF-TOOLBAR` Diff viewer: toolbar, options, scrolling in step, difference navigation centred                                                                                                                                                                                       | T3   | 10 (3 / 7 / 0)                | 24                  | 4334                  | 1616                        | 2     | I-ACTIONS-ENABLED, I-STAGE-HUNKS                                                                                                                                                                                                                                               |
| 118 | `F-VCS-DIR-COMPARE` Directory and folder comparison window                                                                                                                                                                                                                                 | T3   | 8 (8 / 0 / 0)                 | 14                  | 3434                  | 1160                        | 1     | —                                                                                                                                                                                                                                                                              |
| 119 | `F-VCS-STASH` Stash Selected Files, multi-root stashes, Unstash view drops, stash drop                                                                                                                                                                                                     | T3   | 8 (6 / 2 / 0)                 | 14                  | 1706                  | 956                         | 1     | Q-FU4                                                                                                                                                                                                                                                                          |
| 120 | `F-EDITOR-NAVBAR` Navigation bar and breadcrumbs (top, status bar, hidden), nav children                                                                                                                                                                                                   | T3   | 8 (7 / 1 / 0)                 | 2                   | 181                   | 783                         | 1     | —                                                                                                                                                                                                                                                                              |
| 121 | `F-DOTNET` .NET tool window: discover, build, test, restore, packages                                                                                                                                                                                                                      | T3   | 16 (15 / 0 / 1)               | 2                   | 1077                  | 1859                        | 2     | F-RUN-TYPES-OTHER-A, F-RUN-TYPES-OTHER-B, F-RUN-TYPES-OTHER-C                                                                                                                                                                                                                  |
| 122 | `F-AG-MCP` Agents' MCP servers: settings page, per-session MCP picker, import, discover, test (stand-in server)                                                                                                                                                                            | T3   | 15 (15 / 0 / 0)               | 12                  | 2822                  | 1749                        | 2     | Q-FU3                                                                                                                                                                                                                                                                          |
| 123 | `F-DB-RESULT-TABS` Database result tabs: one tab per result set, pin, rename, close variants, paging, request lifecycle, in-editor results, averages                                                                                                                                       | T3   | 30 (24 / 6 / 0)               | 129                 | 5878                  | 4843                        | 4     | I-ACTIONS-ENABLED, I-TW-CONTENT-TABS, Q-FU4                                                                                                                                                                                                                                    |
| 124 | `F-LSP-SEMANTIC` Semantic highlighting and syntax fallback (64 cases), syntax errors, temp highlight, usage highlight                                                                                                                                                                      | T3   | 22 (16 / 3 / 3)               | 104                 | 6430                  | 3273                        | 3     | —                                                                                                                                                                                                                                                                              |
| 125 | `F-VCS-COMBINED-DIFF` Combined Changes Diff (all files in one diff view)                                                                                                                                                                                                                   | T3   | 7 (5 / 2 / 0)                 | 24                  | 924                   | 1021                        | 1     | —                                                                                                                                                                                                                                                                              |
| 126 | `F-EDITOR-RENDERED-DOCS` Rendered doc comments, Markdown images, rendered view, sticky scroll, relative line numbers, caret row, corner inset, selection layer and marks                                                                                                                   | T3   | 21 (20 / 0 / 1)               | 136                 | 4173                  | 3614                        | 3     | —                                                                                                                                                                                                                                                                              |
| 127 | `F-SET-PYTHON-PHP-B` Python interpreters (status, add interpreter, container interpreter, docstrings) and PHP language level: PHP language level, Xdebug page                                                                                                                              | T3   | 7 (5 / 2 / 0)                 | 13                  | 360                   | 684                         | 1     | —                                                                                                                                                                                                                                                                              |
| 128 | `F-BT-SBT-ANALYZER-A` sbt tool window, Maven/Gradle Dependency Analyzer, Add/Unlink Maven projects, generate sources, skip tests, auto-reload, first import, profile reload: Dependency Analyzer (Maven, Gradle) and Maven project linking                                                 | T3   | 14 (13 / 1 / 0)               | 29                  | 2120                  | 1792                        | 2     | —                                                                                                                                                                                                                                                                              |
| 129 | `F-BT-TAIL` Build tools: the feature, Maven/Gradle tool windows, Execute popup and remaining rows                                                                                                                                                                                          | T3   | 8 (1 / 7 / 0)                 | 0                   | 577                   | 441                         | 1     | F-BT-POM, F-BT-SBT-ANALYZER-A, F-BT-SBT-ANALYZER-B                                                                                                                                                                                                                             |
| 130 | `F-AG-TAIL` Agents: the feature, tool window, editor kinds and remaining rows                                                                                                                                                                                                              | T3   | 7 (5 / 2 / 0)                 | 0                   | 338                   | 568                         | 1     | F-AG-COMPOSER-TASKS-A, F-AG-COMPOSER-TASKS-B, F-AG-COMPOSER-TASKS-C, F-AG-MCP, F-AG-MODELS, F-AG-SESSION-FEATURES                                                                                                                                                              |
| 131 | `I-ZOOM-PX` UI zoom reaches spacing and sizes written in pixels (~2,600 places, mechanical `theme::z(px)` sweep per directory)                                                                                                                                                             | T2   | 8 (2 / 6 / 0)                 | 6                   | 772                   | 2600                        | 2     | —                                                                                                                                                                                                                                                                              |
| 132 | `F-PL-TAIL` Plugins: the feature and the remaining rows                                                                                                                                                                                                                                    | T3   | 4 (1 / 3 / 0)                 | 10                  | 1923                  | 673                         | 1     | F-PL-DETAILS, F-PL-NAV-EVENTS, F-PL-VIEWS-A, F-PL-VIEWS-B, F-PL-VIEWS-C                                                                                                                                                                                                        |
| 133 | `F-DEPLOY-WINDOW` Deployment: window of its own, Download Remote Into, Remote Host, configure, upload to                                                                                                                                                                                   | T3   | 13 (4 / 9 / 0)                | 4                   | 4325                  | 1800                        | 2     | Q-RC, T1-DEPLOY-FTP                                                                                                                                                                                                                                                            |
| 134 | `F-RUN-TABS` Run and Debug tabs: pin, close tab / others / all, pinned by default (run.pinTabs), rerun keeps its pin                                                                                                                                                                       | T3   | 13 (11 / 2 / 0)               | 4                   | 1644                  | 1707                        | 2     | I-ACTIONS-ENABLED, I-TW-CONTENT-TABS, T1-RUN-GUARDS                                                                                                                                                                                                                            |
| 135 | `F-TOOLS-SERVERS` Tools: Language Servers page, install prompts (listed, never run), expired tools, JDK tool                                                                                                                                                                               | T3   | 11 (11 / 0 / 0)               | 51                  | 4156                  | 1983                        | 2     | —                                                                                                                                                                                                                                                                              |
| 136 | `I-SOFTWRAP` Soft wrap in the editor (View › Soft-Wrap, gutter menu row, soft-wrap surfaces)                                                                                                                                                                                               | T2   | 5 (4 / 1 / 0)                 | 0                   | 14505                 | 1400                        | 1     | I-ACTIONS-ENABLED                                                                                                                                                                                                                                                              |
| 137 | `F-VCS-SYNC` Root sync, repository-scoped sync, Sync Commits popup, status merge                                                                                                                                                                                                           | T3   | 9 (8 / 0 / 1)                 | 61                  | 1598                  | 1687                        | 2     | —                                                                                                                                                                                                                                                                              |
| 138 | `F-APP-NEW-PROJECT` New Project wizard with scaffolds (catalog, options, requirements, create), new project template, rename project                                                                                                                                                       | T3   | 9 (7 / 2 / 0)                 | 3                   | 700                   | 1600                        | 2     | —                                                                                                                                                                                                                                                                              |
| 139 | `F-VCS-STAGING` Git staging area (Enable staging area setting, staged/unstaged trees)                                                                                                                                                                                                      | T3   | 2 (1 / 1 / 0)                 | 10                  | 1659                  | 722                         | 1     | F-VCS-CHANGES                                                                                                                                                                                                                                                                  |
| 140 | `F-VCS-TAIL` Version control: the feature, its tool windows, gutter and remaining rows (closes after the VCS slices)                                                                                                                                                                       | T3   | 11 (2 / 9 / 0)                | 4                   | 1136                  | 717                         | 1     | F-VCS-BRANCHES, F-VCS-CHANGES, F-VCS-COMBINED-DIFF, F-VCS-COMMIT-DIALOG, F-VCS-DIFF-FIND, F-VCS-DIFF-TOOLBAR, F-VCS-DIR-COMPARE, F-VCS-HISTORY, F-VCS-LOG, F-VCS-MERGE, F-VCS-REMOTE-LINKS, F-VCS-REPO-DIALOGS-A, F-VCS-REPO-DIALOGS-B, F-VCS-STAGING, F-VCS-STASH, F-VCS-SYNC |
| 141 | `F-SET-TAIL` Settings: the feature row and the remaining Settings rows                                                                                                                                                                                                                     | T3   | 2 (0 / 2 / 0)                 | 0                   | 263                   | 122                         | 1     | F-SET-FILE-TYPES, F-SET-JDK-LANGS, F-SET-PYTHON-PHP-A, F-SET-PYTHON-PHP-B, F-SET-UPSTREAM-TOGGLES-A, F-SET-UPSTREAM-TOGGLES-B                                                                                                                                                  |
| 142 | `F-DB-TAIL` Database: the feature, tool window and remaining rows                                                                                                                                                                                                                          | T3   | 2 (1 / 1 / 0)                 | 0                   | 387                   | 165                         | 1     | F-DB-CONSOLES, F-DB-GRID-A, F-DB-GRID-B, F-DB-RESULT-TABS, F-DB-TREE-SOURCES-A, F-DB-TREE-SOURCES-B                                                                                                                                                                            |
| 143 | `I-FILE-PROMPT` Folder and file prompts with the webview's title and start folder (choosePath)                                                                                                                                                                                             | T2   | 3 (3 / 0 / 0)                 | 1                   | 104                   | 168                         | 1     | —                                                                                                                                                                                                                                                                              |
| 144 | `F-EXTERNAL-TOOLS` External tools as actions                                                                                                                                                                                                                                               | T3   | 1 (1 / 0 / 0)                 | 0                   | 189                   | 186                         | 1     | —                                                                                                                                                                                                                                                                              |
| 145 | `F-APP-TAIL` App: the feature row and remaining rows                                                                                                                                                                                                                                       | T3   | 1 (1 / 0 / 0)                 | 0                   | 71                    | 24                          | 1     | F-APP-NEW-PROJECT, F-APP-SHELL-A, F-APP-SHELL-B, F-APP-SHELL-C, F-APP-WORKSPACE                                                                                                                                                                                                |
| 146 | `V-S1` Vim engine S1 keys and keymap                                                                                                                                                                                                                                                       | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1200                        | 1     | —                                                                                                                                                                                                                                                                              |
| 147 | `V-S2` Vim engine S2 state and host                                                                                                                                                                                                                                                        | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1000                        | 1     | V-S1                                                                                                                                                                                                                                                                           |
| 148 | `V-S3` Vim engine S3 motions I                                                                                                                                                                                                                                                             | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1300                        | 1     | V-S2                                                                                                                                                                                                                                                                           |
| 149 | `V-S4` Vim engine S4 motions II                                                                                                                                                                                                                                                            | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1200                        | 1     | V-S3                                                                                                                                                                                                                                                                           |
| 150 | `V-S5` Vim engine S5 operators                                                                                                                                                                                                                                                             | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1400                        | 1     | V-S3                                                                                                                                                                                                                                                                           |
| 151 | `V-S6` Vim engine S6 registers and paste                                                                                                                                                                                                                                                   | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1000                        | 1     | V-S5                                                                                                                                                                                                                                                                           |
| 152 | `V-S7` Vim engine S7 text objects (+ af/if from recheck)                                                                                                                                                                                                                                   | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1000                        | 1     | V-S5                                                                                                                                                                                                                                                                           |
| 153 | `V-S8` Vim engine S8 insert and replace                                                                                                                                                                                                                                                    | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1200                        | 1     | V-S5, V-S6                                                                                                                                                                                                                                                                     |
| 154 | `V-S9` Vim engine S9 repeat, macros, marks, jumps                                                                                                                                                                                                                                          | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1300                        | 1     | V-S8                                                                                                                                                                                                                                                                           |
| 155 | `V-S11` Vim engine S11 search                                                                                                                                                                                                                                                              | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1200                        | 1     | V-S9                                                                                                                                                                                                                                                                           |
| 156 | `V-S12` Vim engine S12 Ex I                                                                                                                                                                                                                                                                | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1400                        | 1     | V-S11, V-S9                                                                                                                                                                                                                                                                    |
| 157 | `V-S13` Vim engine S13 Ex II substitute and global                                                                                                                                                                                                                                         | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1100                        | 1     | V-S12                                                                                                                                                                                                                                                                          |
| 158 | `V-S10` Vim engine S10 visual                                                                                                                                                                                                                                                              | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1400                        | 1     | V-S6, V-S7, V-S8                                                                                                                                                                                                                                                               |
| 159 | `V-S14` Vim engine S14 multi-cursor and numbers                                                                                                                                                                                                                                            | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 800                         | 1     | V-S10                                                                                                                                                                                                                                                                          |
| 160 | `V-I1` Vim I1 editor seam                                                                                                                                                                                                                                                                  | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 1000                        | 1     | I-SOFTWRAP, V-S1, V-S2, V-S3, V-S4, V-S5                                                                                                                                                                                                                                       |
| 161 | `V-I2A` Vim I2a mode reporting                                                                                                                                                                                                                                                             | V    | 3 (3 / 0 / 0)                 | 0                   | 124                   | 500                         | 1     | V-I1                                                                                                                                                                                                                                                                           |
| 162 | `V-I3` Vim I3 app wiring (toggle, reload rc, every editor gets an attachment)                                                                                                                                                                                                              | V    | 6 (3 / 3 / 0)                 | 9                   | 816                   | 800                         | 1     | V-I1, V-I2A, V-S1                                                                                                                                                                                                                                                              |
| 163 | `V-I2B` Vim I2b command line, messages, search highlight                                                                                                                                                                                                                                   | V    | 0 (0 / 0 / 0)                 | 0                   | 479                   | 900                         | 1     | V-I1, V-I3, V-S11, V-S12, V-S13                                                                                                                                                                                                                                                |
| 164 | `V-I4` Vim I4 IME, clipboard, the remaining webview cases                                                                                                                                                                                                                                  | V    | 4 (3 / 1 / 0)                 | 16                  | 1261                  | 900                         | 1     | V-I1, V-I2A, V-I2B, V-I3, V-S14                                                                                                                                                                                                                                                |

### 1. `T1-AGENT-UNDO` An agent's workspace edit is recorded for Edit › Undo (dropped today)

- Webview sources (lines): `src/features/code-insight/workspaceUndo.ts` (59), `src/editor/core/model/workspaceHistory.ts` (177); spec files 71 lines.
- Native neighbours: `app-gpui/src/workbench/editor_language.rs`, `app-gpui/src/code_insight/workspace_undo.rs`, `app-gpui/src/editor_model` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `evt:WorkspaceEditApplied`, `unit:src/features/code-insight/workspaceUndo.test.ts`.
- Spec files and cases (total cases at HEAD; 1 still to prove): `src/features/code-insight/workspaceUndo.test.ts` 4.
- Never in tests or the real window: AGENT (the edit comes from a test event, no agent starts) (AGENT: never start a coding agent).
- Size (Hypothesis): read about 307 lines, write about 250; one writer.
- Depends on: none.
- Note: Measured at HEAD: `editor_language.rs` matches `WorkspaceEditApplied { by_agent: false }` only, so an agent's edit never reaches `workspace_edit_applied`.

### 2. `T1-RUN-GUARDS` Rerun stops live debug sessions; Debug on a running configuration asks; idle tab claimed

- Webview sources (lines): `src/features/run/store.ts` (1746), `src/features/run/index.ts` (904), `src/features/run/runTabs.ts` (95); spec files 257 lines.
- Native neighbours: `app-gpui/src/run/store.rs`, `app-gpui/src/workbench/debug.rs`, `app-gpui/src/run/driver.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:run.debugConfig`, `act:run.runConfig`, `unit:src/features/run/closeRunTab.test.ts`, `unit:src/features/run/resolveCancellation.test.ts`, `unit:src/features/run/runTabReuse.test.ts`.
- Spec files and cases (total cases at HEAD; 6 still to prove): `src/features/run/closeRunTab.test.ts` 1, `src/features/run/resolveCancellation.test.ts` 3, `src/features/run/runTabReuse.test.ts` 4.
- Never in tests or the real window: TOOL (stand-in runner), DEBUG (stand-in adapter, needs python3 as today) (TOOL: no machine toolchain or real external program; DEBUG: stand-in debug adapter only).
- Size (Hypothesis): read about 3002 lines, write about 1128; one writer.
- Depends on: Q-FU3 (request id on start).

### 3. `T1-TEMPLATES-UP` Templates follow upstream: Enter in a completion's snippet, Java .var through Introduce Variable, Zig postfix/surrounders

- Webview sources (lines): `src/features/templates/editor.ts` (633), `src/features/templates/postfix/model.ts` (604), `src/features/templates/live/surrounders.ts` (245); spec files 456 lines.
- Native neighbours: `app-gpui/src/templates/postfix_model.rs`, `app-gpui/src/templates/live_surrounders.rs`, `app-gpui/src/workbench/editor_templates.rs`, `app-gpui/src/templates/expand.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `set:editor.general.postfixCompletion`, `unit:src/features/templates/__tests__/editor.test.ts`, `unit:src/features/templates/__tests__/postfix.test.ts`, `unit:src/features/templates/__tests__/surrounders.test.ts`.
- Spec files and cases (total cases at HEAD; 6 still to prove): `src/features/templates/__tests__/editor.test.ts` 17, `src/features/templates/__tests__/postfix.test.ts` 10, `src/features/templates/__tests__/surrounders.test.ts` 4.
- Never in tests or the real window: none beyond the usual temp project.
- Size (Hypothesis): read about 1938 lines, write about 656; one writer.
- Depends on: Q-NFT (shares templates/).
- Note: recheck_after_merge.md section 5, M1 to M5 (M6/M7 are in Q-NFT as C5/C4; M8 goes with Emmet expansion).

### 4. `T1-PLUGIN-VISUALS` Plugin decorations, line markers and colours follow upstream; the line-marker gutter is reached by the app

- Webview sources (lines): `src/features/plugins/decorations.ts` (268), `src/features/plugins/lineMarkerGutter.ts` (201), `src/features/plugins/markerStore.ts` (83), `src/features/color-preview/pluginColors.ts` (146), `src/features/plugins/consent.tsx` (180), `src/editor/core/cm/iconArea.ts` (159), `src/features/color-preview/gutter.ts` (123); spec files 673 lines.
- Native neighbours: `app-gpui/src/plugin_models/decorations.rs`, `app-gpui/src/plugin_models/line_markers.rs`, `app-gpui/src/plugin_models/marker_store.rs`, `app-gpui/src/features_b/color_preview`, `app-gpui/src/plugin_models/consent.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:colorPreview.gutter`, `pt:plugins.decorations`, `pt:plugins.indexedMarkers`, `pt:plugins.lineMarkers`, `ovl:plugins.consent`, `ipc:plugin_color_presentations`, `ipc:plugin_decoration_sets`, `ipc:plugin_line_markers`, `unit:src/editor/core/cm/iconArea.test.ts`, `unit:src/features/color-preview/gutter.test.ts`, `unit:src/features/color-preview/pluginColors.test.ts`, `unit:src/features/plugins/__tests__/decorations.test.ts`, `unit:src/features/plugins/lineMarkerGutter.test.ts`, `unit:src/features/plugins/markerStore.test.ts`.
- Spec files and cases (total cases at HEAD; 20 still to prove): `src/editor/core/cm/iconArea.test.ts` 7, `src/features/color-preview/gutter.test.ts` 4, `src/features/color-preview/pluginColors.test.ts` 1, `src/features/plugins/__tests__/decorations.test.ts` 7, `src/features/plugins/lineMarkerGutter.test.ts` 3, `src/features/plugins/markerStore.test.ts` 3.
- Never in tests or the real window: no plugin downloads; the test plugins are built in the test from the repository's fixtures.
- Size (Hypothesis): read about 1833 lines, write about 1216; one writer.
- Depends on: Q-FU2 (marker icons and indexed markers first).
- Note: RETRIAGE_UNIT_2: nothing constructs `LineMarkerGutter` (model only in effect); `plugin_colors` test asserts the removed alpha flow.

### 5. `T1-DEPLOY-FTP` Deployment servers page: FTP/FTPS fields, FTP password staging, server groups (a native removal leaves dangling group members)

- Webview sources (lines): `src/features/settings/pages/deployment.tsx` (1000), `src/features/settings/pages/deploymentModel.ts` (122), `src/platform/remoteSettings.ts` (316), `src/features/settings/pages/sshConnection.ts` (26); spec files 536 lines.
- Native neighbours: `app-gpui/src/settings_view/pages/build_deployment.rs`, `app-gpui/src/settings_view/pages/build_deployment`, `app-gpui/src/platform_more/remote_settings.rs`, `app-gpui/src/settings_model/ssh_connection.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `ipc:deploy_test_connection`, `ipc:ssh_password_saved`, `unit:src/features/settings/__tests__/deploymentFtpPassword.test.tsx`, `unit:src/features/settings/pages/deploymentModel.test.ts`, `unit:src/platform/deploymentSettings.test.ts`, `e2e:e2e/deployment.spec.ts`.
- Spec files and cases (total cases at HEAD; 12 still to prove): `e2e/deployment.spec.ts` 4, `src/features/settings/__tests__/deploymentFtpPassword.test.tsx` 2, `src/features/settings/pages/deploymentModel.test.ts` 12, `src/platform/deploymentSettings.test.ts` 5.
- Never in tests or the real window: NET (no FTP connection: a refused loopback port or a stand-in), SECRET (typed password never in Debug or failure text; secret store is the test home's file vault), WIN (never Test Connection on anything but a local folder) (NET: no network; SECRET: secret values never in Debug, logs or failure text; WIN: the real window only through the inspection script, neutral paths).
- Size (Hypothesis): read about 2000 lines, write about 818; one writer.
- Depends on: Q-RC (the chooser adds the FTP arms first).
- Note: recheck_after_merge.md section 6, mismatches 1 to 9 off the chooser's path.

### 6. `T1-SAFETY-CHECKS` Owed safety checks with no row: Restricted Mode writes, Restart keeps the isolated home, tests off the user's global git ignore and mise config

- Webview sources (lines): `src/features/run/store.ts` (1746), `src/features/vcs/changelists.ts` (256); spec files 0 lines.
- Native neighbours: `app-gpui/src/run/driver.rs`, `app-gpui/src/vcs_ops/patches.rs`, `app-gpui/src/app_models/shell.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: HOME (the point of the slice), GIT (HOME: nothing of the user's home; GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 2002 lines, write about 300; one writer.
- Depends on: none.
- Note: OWED lines 14, 18 (a plugin secret in a launched app goes to the isolated home's file), 20, 21, 32 (the terminal runs the login shell with the real home even under isolation), 85-90. Hypothesis (read at HEAD): `ide_host::ops::run::save_configs` has no trust check and the native list is empty when untrusted as in the webview (`store.ts:225`), so the run-configuration half may be parity; a test settles it. Small; ride with the next integration.

### 7. `F-SEARCH-B` Search: Search Everywhere, recent files and locations, file structure, bookmarks, dialogs

- Webview sources (lines): see the spec files; spec files 980 lines.
- Native neighbours: `app-gpui/src/find`, `app-gpui/src/everywhere` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:navigate.fileStructure`, `act:navigate.findAction`, `act:navigate.gotoFile`, `act:navigate.gotoSymbol`, `act:navigate.recentFiles`, `act:navigate.recentLocations`, `act:navigate.searchEverywhere`, `act:navigate.searchText`, `act:navigate.switcher`, `pt:search.bookmarks`, `pt:(at) src/features/search/everywhere/SearchEverywhere.tsx:86`, `pt:search.bookmarks`, `pt:search.bookmarks.files`, `pt:search.structure.selectIn`, `ovl:search.everywhere`, `ovl:search.fileStructure`, `ovl:search.recent`, `tw:bookmarks`, `tw:structure`, `feat:search`, `unit:src/features/search/__tests__/dialogs.test.tsx`, `unit:src/features/search/__tests__/fileStructure.test.tsx`, `unit:src/features/search/__tests__/inherited.test.ts`, `unit:src/features/search/__tests__/searchKeys.test.ts`, `e2e:e2e/searchEverywhereLayout.spec.ts`.
- Spec files and cases (total cases at HEAD; 25 still to prove): `e2e/searchEverywhereLayout.spec.ts` 5, `src/features/search/__tests__/dialogs.test.tsx` 23, `src/features/search/__tests__/fileStructure.test.tsx` 5, `src/features/search/__tests__/inherited.test.ts` 3, `src/features/search/__tests__/searchKeys.test.ts` 1.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 980 lines, write about 1140; one writer.
- Depends on: I-UNDO-PER-DOC (previews).

### 8. `F-SET-JDK-LANGS` JDK pages (add from disk, jdtls JDK, Download JDK as a listed-not-run flow), Java formatter, Go build tags, CMake, Zig, Node/TS/Ruby/Rust/Go pages, module overrides, project dependencies, Project Structure

- Webview sources (lines): `src/features/settings/pages/languages.tsx` (290), `src/features/settings/pages/builtins.tsx` (83); spec files 1017 lines.
- Native neighbours: `app-gpui/src/settings_view/pages`, `app-gpui/src/settings_model/builtin_pages` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:file.projectStructure`, `ovl:settings.projectStructure`, `set:build.gradle`, `set:build.maven`, `set:languages.go`, `set:languages.java`, `set:languages.node`, `set:languages.ruby`, `set:languages.rust`, `set:languages.typescript`, `set:languages.zig`, `ipc:jdtls_required_java`, `ipc:sdk_jdk_at`, `unit:src/features/settings/__tests__/addJdkFromDisk.test.tsx`, `unit:src/features/settings/__tests__/cmake.test.ts`, `unit:src/features/settings/__tests__/downloadJdk.test.tsx`, `unit:src/features/settings/__tests__/goBuildTags.test.ts`, `unit:src/features/settings/__tests__/javaFormatter.test.tsx`, `unit:src/features/settings/__tests__/jdtlsJdk.test.tsx`, `unit:src/features/settings/__tests__/languages.test.ts`, `unit:src/features/settings/__tests__/moduleOverrides.test.tsx`, `unit:src/features/settings/__tests__/projectDependencies.test.ts`, `unit:src/features/settings/__tests__/zigSettings.test.tsx`.
- Spec files and cases (total cases at HEAD; 30 still to prove): `src/features/settings/__tests__/addJdkFromDisk.test.tsx` 3, `src/features/settings/__tests__/cmake.test.ts` 3, `src/features/settings/__tests__/downloadJdk.test.tsx` 2, `src/features/settings/__tests__/goBuildTags.test.ts` 2, `src/features/settings/__tests__/javaFormatter.test.tsx` 2, `src/features/settings/__tests__/jdtlsJdk.test.tsx` 3, `src/features/settings/__tests__/languages.test.ts` 20, `src/features/settings/__tests__/moduleOverrides.test.tsx` 3, `src/features/settings/__tests__/projectDependencies.test.ts` 4, `src/features/settings/__tests__/zigSettings.test.tsx` 2.
- Never in tests or the real window: TOOL (no JDK started), NET (Download JDK: a stand-in list, nothing downloaded) (NET: no network; TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 1390 lines, write about 1390; one writer.
- Depends on: none.

### 9. `F-SET-UPSTREAM-TOGGLES-B` Settings pages gain upstream's new settings (system notifications, debugger JS / live watches, files in associated apps, terminal highlight, project-view branch, dates): Keymap page, assign shortcut, system hotkeys, page registry, layout e2e specs

- Webview sources (lines): see the spec files; spec files 1325 lines.
- Native neighbours: `app-gpui/src/settings_model/builtin_pages`, `app-gpui/src/settings_view/pages` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:file.settings`, `act:settings.assignShortcut`, `act:settings.keymap`, `ovl:settings.assignShortcut`, `ovl:settings.dialog`, `ipc:app_update_status`, `ipc:settings_set`, `ipc:system_probe_hotkeys`, `ipc:window_open_settings`, `unit:src/features/settings/__tests__/keymap.test.ts`, `unit:src/features/settings/__tests__/pages.test.ts`, `unit:src/features/settings/__tests__/settingsWindowOpen.test.ts`, `unit:src/features/settings/__tests__/systemHotkeys.test.ts`, `e2e:e2e/compactSettingsControls.spec.ts`, `e2e:e2e/postfixRowLayout.spec.ts`, `e2e:e2e/sdkFieldLayout.spec.ts`, `e2e:e2e/selectPopupLayout.spec.ts`, `e2e:e2e/settings.spec.ts`, `e2e:e2e/settingsFormLayout.spec.ts`, `e2e:e2e/settingsHeaderLayout.spec.ts`, `e2e:e2e/settingsListLayout.spec.ts`, `e2e:e2e/settingsPaneLayout.spec.ts`.
- Spec files and cases (total cases at HEAD; 30 still to prove): `e2e/compactSettingsControls.spec.ts` 2, `e2e/postfixRowLayout.spec.ts` 1, `e2e/sdkFieldLayout.spec.ts` 2, `e2e/selectPopupLayout.spec.ts` 1, `e2e/settings.spec.ts` 5, `e2e/settingsFormLayout.spec.ts` 3, `e2e/settingsHeaderLayout.spec.ts` 1, `e2e/settingsListLayout.spec.ts` 1, `e2e/settingsPaneLayout.spec.ts` 4, `src/features/settings/__tests__/keymap.test.ts` 23, `src/features/settings/__tests__/pages.test.ts` 7, `src/features/settings/__tests__/settingsWindowOpen.test.ts` 1, `src/features/settings/__tests__/systemHotkeys.test.ts` 3.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 1325 lines, write about 990; one writer.
- Depends on: none.

### 10. `I-TW-CONTENT-TABS` Content tabs in a tool window (closable, pinnable, tab menu) on the shared header

- Webview sources (lines): `src/workbench/toolwindows/content.ts` (32), `src/workbench/toolwindows/ToolWindows.tsx` (1376); spec files 282 lines.
- Native neighbours: `app-gpui/src/workbench/tool_windows.rs`, `app-gpui/src/workbench/chrome.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:toolwindow.closeContent`, `act:toolwindow.viewMode.dockPinned`, `act:toolwindow.viewMode.dockUnpinned`, `act:toolwindow.viewMode.undock`, `mod:src/workbench/toolwindows/content.ts`, `unit:src/workbench/toolwindows/badges.test.tsx`, `unit:src/workbench/toolwindows/content.test.ts`, `unit:src/workbench/toolwindows/header.test.tsx`, `e2e:e2e/toolWindowViewMode.spec.ts`.
- Spec files and cases (total cases at HEAD; 13 still to prove): `e2e/toolWindowViewMode.spec.ts` 3, `src/workbench/toolwindows/badges.test.tsx` 1, `src/workbench/toolwindows/content.test.ts` 2, `src/workbench/toolwindows/header.test.tsx` 8.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 1690 lines, write about 1236; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 78 %; Hypothesis).
- Depends on: Q-VIS2 (int_visual2_brief.md item 2 gives the shared header its content slot).
- Note: Needed by Hierarchy tabs, pinned Run/Debug tabs, Database result tabs, plugin view windows.

### 11. `F-HIERARCHY-USAGES` Hierarchy tabs (pin, root detail, prefetched levels), Find Usages popup and results, problems, server status, ESLint, type hierarchy prepare

- Webview sources (lines): `src/features/code-insight` (3720), `src/features/code-insight/ServerStatus.tsx` (141), `src/features/code-insight/hierarchy.ts` (520), `src/features/code-insight/problemsStore.ts` (212), `src/features/code-insight/servers.ts` (229), `src/features/code-insight/usagesPopup.tsx` (313); spec files 1661 lines.
- Native neighbours: `app-gpui/src/code_insight` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:find.usages`, `act:navigate.callHierarchy`, `act:navigate.typeHierarchy`, `pt:problems`, `pt:codeInsight.eslint`, `ovl:code-insight.usagesPopup`, `stat:code-insight.servers`, `tw:hierarchy`, `tw:problems`, `tw:usages`, `feat:code-insight`, `ipc:lsp_prepare_type_hierarchy`, `unit:src/features/code-insight/hierarchy.test.ts`, `unit:src/features/code-insight/problemsStore.test.ts`, `unit:src/features/code-insight/servers.test.ts`, `unit:src/features/code-insight/ServerStatus.test.tsx`, `unit:src/features/code-insight/usagePreview.test.tsx`, `unit:src/features/code-insight/usageResults.test.ts`, `unit:src/features/code-insight/usagesPopup.test.tsx`, `e2e:e2e/findUsages-ui.spec.ts`.
- Spec files and cases (total cases at HEAD; 22 still to prove): `e2e/findUsages-ui.spec.ts` 2, `src/features/code-insight/ServerStatus.test.tsx` 5, `src/features/code-insight/hierarchy.test.ts` 12, `src/features/code-insight/problemsStore.test.ts` 10, `src/features/code-insight/servers.test.ts` 8, `src/features/code-insight/usagePreview.test.tsx` 3, `src/features/code-insight/usageResults.test.ts` 10, `src/features/code-insight/usagesPopup.test.tsx` 16.
- Never in tests or the real window: TOOL (stand-in language server) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 6796 lines, write about 1497; one writer.
- Depends on: I-TW-CONTENT-TABS, Q-FU3 (slices: I-TW-CONTENT-TABS).

### 12. `F-PLAT-PROJECT-A` Platform project models: project, roots, presentation, trust model, remote, virtual docs, class files, file manager, file ops

- Webview sources (lines): `src/platform/fileManager.ts` (93), `src/platform/fileOps.ts` (47), `src/platform/libraries.ts` (17), `src/platform/project.ts` (137), `src/platform/projectPresentation.ts` (88), `src/platform/projectRoots.ts` (173), `src/platform/projectTrust.ts` (20), `src/platform/remote.ts` (380), `src/platform/remoteSettings.ts` (316), `src/platform/virtualDocs.ts` (169), `src/platform/classFiles.ts` (8); spec files 509 lines.
- Native neighbours: `app-gpui/src/platform_more/project.rs`, `app-gpui/src/platform/project_roots.rs`, `app-gpui/src/platform/virtual_docs.rs`, `app-gpui/src/platform_more/remote.rs`, `app-gpui/src/platform_more/dotnet.rs`, `app-gpui/src/platform_more/build_tool_recovery.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `mod:src/platform/fileManager.ts`, `mod:src/platform/fileOps.ts`, `mod:src/platform/libraries.ts`, `mod:src/platform/project.ts`, `mod:src/platform/projectPresentation.ts`, `mod:src/platform/projectRoots.ts`, `mod:src/platform/projectTrust.ts`, `mod:src/platform/remote.ts`, `mod:src/platform/remoteSettings.ts`, `mod:src/platform/virtualDocs.ts`, `ipc:fs_open_with_system`, `ipc:remote_current`, `ipc:remote_invoke`, `evt:ProjectRootsChanged`, `unit:src/platform/classFiles.test.ts`, `unit:src/platform/fileManager.test.ts`, `unit:src/platform/project.test.ts`, `unit:src/platform/remote.test.ts`, `unit:src/platform/virtualDocs.test.ts`.
- Spec files and cases (total cases at HEAD; 15 still to prove): `src/platform/classFiles.test.ts` 3, `src/platform/fileManager.test.ts` 12, `src/platform/project.test.ts` 6, `src/platform/remote.test.ts` 7, `src/platform/virtualDocs.test.ts` 7.
- Never in tests or the real window: HOME, TOOL (no JDK or dotnet is started: the lookup's process seam answers) (TOOL: no machine toolchain or real external program; HOME: nothing of the user's home).
- Size (Hypothesis): read about 1957 lines, write about 1466; one writer.
- Depends on: none.

### 13. `F-SET-UPSTREAM-TOGGLES-A` Settings pages gain upstream's new settings (system notifications, debugger JS / live watches, files in associated apps, terminal highlight, project-view branch, dates): the pages' new settings (one line each) and the dialog spec

- Webview sources (lines): see the spec files; spec files 1102 lines.
- Native neighbours: `app-gpui/src/settings_model/builtin_pages`, `app-gpui/src/settings_view/pages` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `set:(at) src/features/settings/pages/registry.tsx:64`, `set:(at) src/features/settings/pages/registry.tsx:71`, `set:(at) src/features/settings/pages/registry.tsx:84`, `set:build.debugger`, `set:build.debugger.dataViews`, `set:editor.files`, `set:general`, `set:general.dateFormats`, `set:general.httpProxy`, `set:general.updates`, `set:keymap`, `set:tools.terminal`, `set:vcs.issueNavigation`, `unit:src/features/settings/__tests__/appearanceToggles.test.ts`, `unit:src/features/settings/__tests__/behavior.test.ts`, `unit:src/features/settings/__tests__/dialog.test.tsx`, `unit:src/features/settings/__tests__/mainMenuStyle.test.tsx`, `unit:src/features/settings/__tests__/window.test.ts`.
- Spec files and cases (total cases at HEAD; 23 still to prove): `src/features/settings/__tests__/appearanceToggles.test.ts` 3, `src/features/settings/__tests__/behavior.test.ts` 13, `src/features/settings/__tests__/dialog.test.tsx` 40, `src/features/settings/__tests__/mainMenuStyle.test.tsx` 3, `src/features/settings/__tests__/window.test.ts` 3.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 1102 lines, write about 1074; one writer.
- Depends on: none.

### 14. `I-UNTITLED` Untitled buffers (File › New Untitled; VS Code presets' Cmd/Ctrl+N does nothing natively today)

- Webview sources (lines): `src/editor/core/controller.ts` (2340), `src/editor/core/model/document.ts` (919), `src/features/app/appActions.ts` (111); spec files 171 lines.
- Native neighbours: `app-gpui/src/editor_model`, `app-gpui/src/workbench/edit_actions.rs`, `app-gpui/src/workbench/light_edit_tabs.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:file.newUntitled`, `keymap:vscode-macos`, `keymap:vscode-windows`, `ipc:doc_open_untitled`, `ipc:light_edit_open_untitled`, `unit:src/editor/core/untitled.test.ts`.
- Spec files and cases (total cases at HEAD; 7 still to prove): `src/editor/core/untitled.test.ts` 7.
- Never in tests or the real window: HOME (save prompts into the temp folder) (HOME: nothing of the user's home).
- Size (Hypothesis): read about 3541 lines, write about 900; one writer.
- Depends on: none.

### 15. `F-SMALL-FEATURES-A` Small features: build (compile file, module), web preview, project trust, welcome

- Webview sources (lines): see the spec files; spec files 943 lines.
- Native neighbours: `app-gpui/src/features_b`, `app-gpui/src/workbench/welcome.rs`, `app-gpui/src/features_a/todo.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:build.buildArtifacts`, `act:build.buildModule`, `act:build.buildProject`, `act:build.compileFile`, `act:build.rebuildProject`, `act:file.openRecent`, `act:project.trust`, `act:web.openInBrowser`, `act:web.togglePreview`, `act:welcome.recentProjects`, `pt:build.compileFile`, `pt:build.module`, `pt:web.open`, `pt:web.openIn`, `ovl:project.trustDialog`, `ovl:welcome.recentProjects`, `pt:web.html`, `tw:build`, `pt:project.restrictedMode`, `feat:build`, `feat:project-trust`, `feat:web`, `feat:welcome`, `ipc:build_compile_file`, `ipc:remote_disconnect`, `ipc:remote_trust_set`, `ipc:remote_trust_status`, `ipc:web_goto`, `ipc:web_open`, `ipc:web_unresolved_references`, `ipc:web_url`, `ipc:window_open_projects`, `unit:src/features/build/build.test.tsx`, `unit:src/features/project-trust/trust.test.tsx`, `unit:src/features/welcome/welcome.test.tsx`.
- Spec files and cases (total cases at HEAD; 9 still to prove): `src/features/build/build.test.tsx` 16, `src/features/project-trust/trust.test.tsx` 11, `src/features/welcome/welcome.test.tsx` 11.
- Never in tests or the real window: NET (web preview only on loopback; What's New reads a fixture), TOOL (NET: no network; TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 943 lines, write about 1782; split into 2: one writer per two or three features.
- Depends on: none (slices: I-UNTITLED).

### 16. `F-PLAT-MODELS-C-B` Platform models III: error log, platform info, proxy, workflow, client and editor settings, small platform modules

- Webview sources (lines): `src/platform/agentStatus.ts` (2), `src/platform/clientSettings.ts` (131), `src/platform/cmakeSettings.ts` (38), `src/platform/consoleColumns.ts` (61), `src/platform/editorSettings.ts` (334), `src/platform/errorLog.ts` (54), `src/platform/fileColors.ts` (129), `src/platform/fileTypes.ts` (165), `src/platform/formatGuard.ts` (222), `src/platform/ignoredNames.ts` (50), `src/platform/languageList.ts` (20), `src/platform/languageSettings.ts` (780), `src/platform/listeners.ts` (13), `src/platform/localDiagnostics.ts` (20), `src/platform/logLevels.ts` (3), `src/platform/openUrl.ts` (24), `src/platform/platformInfo.ts` (62), `src/platform/proxySettings.ts` (82), `src/platform/search.ts` (149), `src/platform/textSearch.ts` (49), `src/platform/updateSource.ts` (11), `src/platform/workflowSettings.ts` (697); spec files 360 lines.
- Native neighbours: `app-gpui/src/platform/i18n.rs`, `app-gpui/src/platform/date_format.rs`, `app-gpui/src/platform_more/boot.rs`, `app-gpui/src/platform/error_log_sink.rs`, `app-gpui/src/platform_more/proxy_settings.rs`, `app-gpui/src/platform_more/workflow_settings.rs`, `app-gpui/src/platform_more/client_settings.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `mod:src/platform/agentStatus.ts`, `mod:src/platform/clientSettings.ts`, `mod:src/platform/cmakeSettings.ts`, `mod:src/platform/consoleColumns.ts`, `mod:src/platform/editorSettings.ts`, `mod:src/platform/errorLog.ts`, `mod:src/platform/fileColors.ts`, `mod:src/platform/fileTypes.ts`, `mod:src/platform/formatGuard.ts`, `mod:src/platform/ignoredNames.ts`, `mod:src/platform/languageList.ts`, `mod:src/platform/languageSettings.ts`, `mod:src/platform/listeners.ts`, `mod:src/platform/localDiagnostics.ts`, `mod:src/platform/logLevels.ts`, `mod:src/platform/openUrl.ts`, `mod:src/platform/platformInfo.ts`, `mod:src/platform/proxySettings.ts`, `mod:src/platform/search.ts`, `mod:src/platform/textSearch.ts`, `mod:src/platform/updateSource.ts`, `mod:src/platform/workflowSettings.ts`, `ipc:app_info`, `ipc:project_current`, `ipc:settings_get_global`, `unit:src/platform/consoleColumns.test.ts`, `unit:src/platform/errorLog.test.ts`, `unit:src/platform/localDiagnostics.test.ts`, `unit:src/platform/openUrl.test.ts`, `unit:src/platform/platformInfo.test.ts`, `unit:src/platform/proxySettings.test.ts`, `unit:src/platform/search.test.ts`, `unit:src/platform/workflowSettings.test.ts`.
- Spec files and cases (total cases at HEAD; 13 still to prove): `src/platform/consoleColumns.test.ts` 2, `src/platform/errorLog.test.ts` 2, `src/platform/localDiagnostics.test.ts` 1, `src/platform/openUrl.test.ts` 2, `src/platform/platformInfo.test.ts` 3, `src/platform/proxySettings.test.ts` 6, `src/platform/search.test.ts` 4, `src/platform/workflowSettings.test.ts` 4.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 3456 lines, write about 2667; split into 2: i18n/catalog/date, then the rest.
- Depends on: Q-FU3 (date format item 9).

### 17. `F-SMALL-FEATURES-C` Small features: TODO, Rails, Thymeleaf, remote development and ports, Logcat (device stand-in), window zoom, style specs

- Webview sources (lines): `src/features/logcat/history.ts` (47), `src/features/logcat/model.ts` (34), `src/features/rails/conventions.ts` (153), `src/features/rails/references.ts` (62), `src/features/remote-dev/ConnectDialog.tsx` (252), `src/features/remote-dev/container.ts` (27), `src/features/remote-dev/environment.ts` (64), `src/features/remote-dev/ports.ts` (109), `src/features/thymeleaf/references.ts` (136), `src/features/todo/PreviewEditor.tsx` (163); spec files 1408 lines.
- Native neighbours: `app-gpui/src/features_b`, `app-gpui/src/workbench/welcome.rs`, `app-gpui/src/features_a/todo.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:remote.connect`, `act:remote.disconnect`, `act:remote.reloadEnvironment`, `act:remote.reopenInContainer`, `pt:todo.highlight`, `pt:(at) src/features/rails/index.ts:57`, `pt:(at) src/features/thymeleaf/index.ts:64`, `ovl:remote.connect`, `stat:remote.location`, `tw:logcat`, `tw:remote.ports`, `tw:todo`, `feat:logcat`, `feat:rails`, `feat:remote-dev`, `feat:thymeleaf`, `feat:todo`, `ipc:env_reload`, `ipc:remote_forwarded_ports`, `ipc:remote_stop_forwarding`, `unit:src/features/logcat/history.test.ts`, `unit:src/features/logcat/model.test.ts`, `unit:src/features/rails/conventions.test.ts`, `unit:src/features/rails/references.test.ts`, `unit:src/features/remote-dev/ConnectDialog.test.tsx`, `unit:src/features/remote-dev/container.test.ts`, `unit:src/features/remote-dev/environment.test.ts`, `unit:src/features/remote-dev/ports.test.tsx`, `unit:src/features/remote-dev/remoteDev.test.ts`, `unit:src/features/thymeleaf/references.test.ts`, `unit:src/features/todo/PreviewEditor.test.tsx`, `unit:src/features/todo/todo.test.ts`, `unit:src/styles/spacingScale.test.ts`.
- Spec files and cases (total cases at HEAD; 34 still to prove): `src/features/logcat/history.test.ts` 4, `src/features/logcat/model.test.ts` 15, `src/features/rails/conventions.test.ts` 6, `src/features/rails/references.test.ts` 2, `src/features/remote-dev/ConnectDialog.test.tsx` 5, `src/features/remote-dev/container.test.ts` 3, `src/features/remote-dev/environment.test.ts` 4, `src/features/remote-dev/ports.test.tsx` 9, `src/features/remote-dev/remoteDev.test.ts` 14, `src/features/thymeleaf/references.test.ts` 7, `src/features/todo/PreviewEditor.test.tsx` 7, `src/features/todo/todo.test.ts` 13, `src/styles/spacingScale.test.ts` 1.
- Never in tests or the real window: NET (web preview only on loopback; What's New reads a fixture), TOOL (NET: no network; TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 2455 lines, write about 2328; split into 2: one writer per two or three features.
- Depends on: none.

### 18. `F-VCS-REPO-DIALOGS-A` Git dialogs pick the repository (merge, pull, rebase, tag, push), operation repository, multi-root: dialogs pick the repository: merge, pull, rebase, tag, operation repository, multi-root, branch ops

- Webview sources (lines): `src/features/vcs/branchOps.ts` (760), `src/features/vcs/multiRoot.ts` (58); spec files 1801 lines.
- Native neighbours: `app-gpui/src/vcs/git_forms.rs`, `app-gpui/src/vcs_ops/branch_ops.rs`, `app-gpui/src/vcs_more/push_target.rs`, `app-gpui/src/vcs_more/multi_root.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vcs.compareWithBranch`, `act:vcs.newBranch`, `act:vcs.newTag`, `ovl:vcs.dialogs`, `ipc:vcs_delete_tag`, `ipc:vcs_restore_tag`, `unit:src/features/vcs/__tests__/branchOps.test.ts`, `unit:src/features/vcs/__tests__/deleteBranches.test.ts`, `unit:src/features/vcs/__tests__/gitOps.test.ts`, `unit:src/features/vcs/__tests__/mergeDialogRepo.test.tsx`, `unit:src/features/vcs/__tests__/multiRoot.test.ts`, `unit:src/features/vcs/__tests__/operationRepo.test.ts`, `unit:src/features/vcs/__tests__/promptDialog.test.tsx`, `unit:src/features/vcs/__tests__/pullDialogRepo.test.tsx`, `unit:src/features/vcs/__tests__/rebaseDialogRepo.test.tsx`, `unit:src/features/vcs/__tests__/tagDialogRepo.test.tsx`.
- Spec files and cases (total cases at HEAD; 32 still to prove): `src/features/vcs/__tests__/branchOps.test.ts` 24, `src/features/vcs/__tests__/deleteBranches.test.ts` 11, `src/features/vcs/__tests__/gitOps.test.ts` 15, `src/features/vcs/__tests__/mergeDialogRepo.test.tsx` 3, `src/features/vcs/__tests__/multiRoot.test.ts` 16, `src/features/vcs/__tests__/operationRepo.test.ts` 3, `src/features/vcs/__tests__/promptDialog.test.tsx` 8, `src/features/vcs/__tests__/pullDialogRepo.test.tsx` 3, `src/features/vcs/__tests__/rebaseDialogRepo.test.tsx` 3, `src/features/vcs/__tests__/tagDialogRepo.test.tsx` 4.
- Never in tests or the real window: GIT (a bare repository in the temp folder stands for the remote) (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 2619 lines, write about 1102; one writer.
- Depends on: Q-FU4.

### 19. `F-DBG-CONSOLE-WATCHES-B` Debug console (links, find, history, input to the program), expression completion, live watches, watch storage, event breakpoints, class imports, startup breakpoints: watches, values, evaluation, breakpoints (event, startup), class imports, execution state

- Webview sources (lines): `src/features/debugger/classImports.ts` (202), `src/features/debugger/values.ts` (103); spec files 1466 lines.
- Native neighbours: `app-gpui/src/debugger/variables_view.rs`, `app-gpui/src/debugger/evaluate_dialog.rs`, `app-gpui/src/debugger/state.rs`, `app-gpui/src/debugger/breakpoints` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:debug.evaluate`, `act:debug.quickEvaluate`, `act:debug.smartStepInto`, `pt:debugger.evaluate`, `ipc:lsp_workspace_symbols`, `ipc:run_env_files`, `evt:Debug`, `unit:src/features/debugger/__tests__/classImports.test.ts`, `unit:src/features/debugger/__tests__/eventBreakpoints.test.ts`, `unit:src/features/debugger/__tests__/EventBreakpointsSection.test.tsx`, `unit:src/features/debugger/__tests__/executionState.test.ts`, `unit:src/features/debugger/__tests__/liveWatches.test.ts`, `unit:src/features/debugger/__tests__/quickEvaluate.test.ts`, `unit:src/features/debugger/__tests__/startupBreakpoints.test.ts`, `unit:src/features/debugger/__tests__/values.test.ts`, `unit:src/features/debugger/__tests__/watchStorage.test.ts`.
- Spec files and cases (total cases at HEAD; 49 still to prove): `src/features/debugger/__tests__/EventBreakpointsSection.test.tsx` 1, `src/features/debugger/__tests__/classImports.test.ts` 11, `src/features/debugger/__tests__/eventBreakpoints.test.ts` 11, `src/features/debugger/__tests__/executionState.test.ts` 14, `src/features/debugger/__tests__/liveWatches.test.ts` 9, `src/features/debugger/__tests__/quickEvaluate.test.ts` 9, `src/features/debugger/__tests__/startupBreakpoints.test.ts` 13, `src/features/debugger/__tests__/values.test.ts` 14, `src/features/debugger/__tests__/watchStorage.test.ts` 5.
- Never in tests or the real window: DEBUG (stand-in adapter, python3 as today) (DEBUG: stand-in debug adapter only).
- Size (Hypothesis): read about 1771 lines, write about 1288; one writer.
- Depends on: none.

### 20. `F-WB-SHELL-A` Workbench shell: Workbench, project window and colour, quick switch, decorations, providers, file editors, endpoint providers

- Webview sources (lines): `src/workbench/Workbench.tsx` (123), `src/workbench/browserGuards.ts` (65), `src/workbench/projectWindow.ts` (77), `src/workbench/providerAcquisition.ts` (63), `src/workbench/quickSwitch.ts` (152); spec files 1001 lines.
- Native neighbours: `app-gpui/src/workbench/mod.rs`, `app-gpui/src/workbench/quick_switch.rs`, `app-gpui/src/platform_more/file_drop.rs`, `app-gpui/src/platform_more/file_editors.rs`, `app-gpui/src/platform_more/provider_acquisition.rs`, `app-gpui/src/platform_more/occurrences.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:file.openProject`, `act:navigate.nextOccurrence`, `act:navigate.prevOccurrence`, `ctx:projectAttached`, `ctx:projectLocal`, `ctx:projectOpenAsk`, `ctx:workspaceContainer`, `ctx:workspaceOpen`, `unit:src/workbench/browserGuards.test.ts`, `unit:src/workbench/decorations.test.ts`, `unit:src/workbench/endpointProviders.test.ts`, `unit:src/workbench/fileEditors.test.ts`, `unit:src/workbench/projectWindow.test.ts`, `unit:src/workbench/providerAcquisition.test.ts`, `unit:src/workbench/quickSwitch.test.tsx`, `unit:src/workbench/Workbench.test.tsx`.
- Spec files and cases (total cases at HEAD; 22 still to prove): `src/workbench/Workbench.test.tsx` 19, `src/workbench/browserGuards.test.ts` 5, `src/workbench/decorations.test.ts` 1, `src/workbench/endpointProviders.test.ts` 3, `src/workbench/fileEditors.test.ts` 3, `src/workbench/projectWindow.test.ts` 4, `src/workbench/providerAcquisition.test.ts` 4, `src/workbench/quickSwitch.test.tsx` 5.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 1481 lines, write about 954; one writer.
- Depends on: none.

### 21. `F-APP-SHELL-C` App shell: shell, menu bridge, CLI, log, about, caches, open paths, export/import settings

- Webview sources (lines): `src/features/app/cliCommand.ts` (73), `src/features/app/host.ts` (28), `src/features/app/menuBridge.ts` (292); spec files 600 lines.
- Native neighbours: `app-gpui/src/app_models/close_guard.rs`, `app-gpui/src/app_models/windows.rs`, `app-gpui/src/app_models/shell.rs`, `app-gpui/src/app_models/cli_command.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:app.about`, `act:app.installCli`, `act:app.uninstallCli`, `act:file.exportSettings`, `act:file.importSettings`, `act:file.openPaths`, `act:file.openProjectPath`, `act:file.openRecentProject`, `act:help.showLog`, `pt:app.powerSave`, `ovl:app.about`, `ovl:app.keymapReference`, `shell:src-tauri/src/cli.rs`, `shell:src-tauri/src/logging.rs`, `shell:src-tauri/src/macos.rs`, `shell:src-tauri/src/page_swipe.rs`, `shell:src-tauri/src/relaunch.rs`, `shell:src-tauri/src/window_zoom.rs`, `ipc:app_cli_install`, `ipc:app_cli_uninstall`, `ipc:app_log_location`, `ipc:caches_schedule_invalidation`, `ipc:menu_configure`, `ipc:menu_model`, `ipc:menu_set_disabled`, `ipc:settings_import`, `ipc:window_toggle_zoom`, `unit:src/features/app/__tests__/cliCommand.test.ts`, `unit:src/features/app/__tests__/host.test.ts`, `unit:src/features/app/__tests__/menuBridge.test.ts`, `unit:src/features/app/__tests__/shell.test.tsx`.
- Spec files and cases (total cases at HEAD; 13 still to prove): `src/features/app/__tests__/cliCommand.test.ts` 4, `src/features/app/__tests__/host.test.ts` 3, `src/features/app/__tests__/menuBridge.test.ts` 17, `src/features/app/__tests__/shell.test.tsx` 9.
- Never in tests or the real window: HOME (the CLI install writes only into a temp bin folder; Restart is checked to keep OPENIDEA_GPUI_HOME), NET (no update check) (NET: no network; HOME: nothing of the user's home).
- Size (Hypothesis): read about 993 lines, write about 1871; split into 2: close guard + windows (77 cases), then shell/menu/CLI, then JetBrains import.
- Depends on: Q-FU3 (closeWithoutWelcome).

### 22. `F-LSP-REFACTOR-B` Refactorings through the server: Java move, introduce (variable, constant, field, parameter, method), server introduce, cancellation

- Webview sources (lines): `src/editor/refactor/introduce.ts` (710), `src/editor/refactor/javaMove.ts` (256), `src/editor/refactor/serverIntroduce.ts` (339); spec files 758 lines.
- Native neighbours: `app-gpui/src/editor_refactor`, `app-gpui/src/workbench/refactor_actions.rs`, `app-gpui/src/editor_lsp/workspace_edit.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:refactor.extractConstant`, `act:refactor.extractField`, `act:refactor.extractMethod`, `act:refactor.extractParameter`, `act:refactor.extractVariable`, `mod:src/editor/refactor/introduce.ts`, `mod:src/editor/refactor/javaMove.ts`, `mod:src/editor/refactor/serverIntroduce.ts`, `ipc:lsp_java_move`, `ipc:lsp_java_move_destinations`, `ipc:lsp_java_source_types`, `unit:src/editor/refactor/cancellable.test.ts`, `unit:src/editor/refactor/introduce.test.ts`, `unit:src/editor/refactor/javaMove.test.ts`, `unit:src/editor/refactor/serverIntroduce.test.ts`.
- Spec files and cases (total cases at HEAD; 20 still to prove): `src/editor/refactor/cancellable.test.ts` 4, `src/editor/refactor/introduce.test.ts` 11, `src/editor/refactor/javaMove.test.ts` 3, `src/editor/refactor/serverIntroduce.test.ts` 7.
- Never in tests or the real window: TOOL (stand-in language server answering the refactoring requests) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 2063 lines, write about 1296; one writer.
- Depends on: none.

### 23. `F-LSP-REFACTOR-C` Refactorings through the server: Refactor This, safe delete, run kind, catalog and the rest

- Webview sources (lines): `src/editor/refactor/catalog.ts` (205), `src/editor/refactor/index.ts` (7), `src/editor/refactor/names.ts` (256), `src/editor/refactor/occurrences.ts` (119), `src/editor/refactor/projectLevel.ts` (142), `src/editor/refactor/query.ts` (57), `src/editor/refactor/refactorThis.ts` (343), `src/editor/refactor/safeDelete.ts` (590), `src/editor/refactor/templates.ts` (272); spec files 722 lines.
- Native neighbours: `app-gpui/src/editor_refactor`, `app-gpui/src/workbench/refactor_actions.rs`, `app-gpui/src/editor_lsp/workspace_edit.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:refactor.safeDelete`, `act:refactor.thisMenu`, `mod:src/editor/refactor/catalog.ts`, `mod:src/editor/refactor/index.ts`, `mod:src/editor/refactor/names.ts`, `mod:src/editor/refactor/occurrences.ts`, `mod:src/editor/refactor/projectLevel.ts`, `mod:src/editor/refactor/query.ts`, `mod:src/editor/refactor/refactorThis.ts`, `mod:src/editor/refactor/safeDelete.ts`, `mod:src/editor/refactor/templates.ts`, `ipc:prepare_workspace_edit`, `unit:src/editor/refactor/refactor.test.ts`, `unit:src/editor/refactor/refactorThis.test.ts`, `unit:src/editor/refactor/runKind.test.ts`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/editor/refactor/refactor.test.ts` 19, `src/editor/refactor/refactorThis.test.ts` 17, `src/editor/refactor/runKind.test.ts` 3.
- Never in tests or the real window: TOOL (stand-in language server answering the refactoring requests) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 2713 lines, write about 1338; one writer.
- Depends on: none.

### 24. `F-VIEWERS` Viewers: Markdown preview search highlight, Mermaid full screen/PNG, local SVG, zoom, binary and image editors, associated applications

- Webview sources (lines): `src/features/viewers` (3050), `src/features/viewers/markdown/highlight.ts` (392), `src/features/viewers/markdown/localSvg.ts` (47), `src/features/viewers/markdown/mermaid.ts` (334), `src/features/viewers/markdown/render.ts` (255); spec files 2014 lines.
- Native neighbours: `app-gpui/src/features_b/viewers` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:viewers.markdownLive`, `kind:binary`, `kind:image`, `pt:viewers.associatedApps`, `pt:(at) src/features/viewers/markdown/MarkdownEditor.tsx:390`, `pt:viewers.markdown`, `feat:viewers`, `unit:src/features/viewers/markdown/highlight.test.tsx`, `unit:src/features/viewers/markdown/highlightIndex.test.ts`, `unit:src/features/viewers/markdown/localSvg.test.ts`, `unit:src/features/viewers/markdown/mermaid.test.ts`, `unit:src/features/viewers/markdown/previewScroll.test.tsx`, `unit:src/features/viewers/markdown/render.test.ts`, `unit:src/features/viewers/markdown/zoom.test.tsx`, `unit:src/features/viewers/viewers.test.ts`.
- Spec files and cases (total cases at HEAD; 35 still to prove): `src/features/viewers/markdown/highlight.test.tsx` 7, `src/features/viewers/markdown/highlightIndex.test.ts` 7, `src/features/viewers/markdown/localSvg.test.ts` 3, `src/features/viewers/markdown/mermaid.test.ts` 20, `src/features/viewers/markdown/previewScroll.test.tsx` 3, `src/features/viewers/markdown/render.test.ts` 18, `src/features/viewers/markdown/zoom.test.tsx` 9, `src/features/viewers/viewers.test.ts` 12.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 6092 lines, write about 1382; one writer.
- Depends on: none.

### 25. `F-GH-REVIEW-B` GitHub pull requests: timeline, pull request tool window, create, update/delete branch, the rest

- Webview sources (lines): `src/features/github/clone/GitHubCloneSource.tsx` (176); spec files 612 lines.
- Native neighbours: `app-gpui/src/github` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:github.checkoutPullRequest`, `act:github.pullRequest.checkoutBranch`, `act:github.pullRequest.deleteMergedBranch`, `act:github.pullRequest.refresh`, `act:github.pullRequest.show`, `act:github.pullRequest.showInToolWindow`, `act:github.pullRequest.showTimeline`, `act:github.pullRequest.timeline.refresh`, `act:github.pullRequest.updateBranch`, `act:github.selectOpenedPullRequest`, `ctx:CONNECTED`, `ctx:githubCurrentBranchPullRequest`, `ctx:githubTimelineActive`, `ctx:handlerContextKey`, `pt:github`, `kind:github.timeline`, `pt:(at) src/features/github/index.ts:269`, `ovl:github.dialogs`, `tw:pullRequests`, `feat:github`, `ipc:gh_pr_comment`, `ipc:gh_pr_list`, `ipc:gh_pr_review`, `ipc:gh_pr_review_comments`, `ipc:gh_pr_view`, `unit:src/features/github/clone/GitHubCloneSource.test.tsx`, `unit:src/features/github/create/create.test.ts`, `unit:src/features/github/toolwindow/toolwindow.test.tsx`, `unit:src/features/github/ui/ui.test.tsx`.
- Spec files and cases (total cases at HEAD; 12 still to prove): `src/features/github/clone/GitHubCloneSource.test.tsx` 3, `src/features/github/create/create.test.ts` 15, `src/features/github/toolwindow/toolwindow.test.tsx` 8, `src/features/github/ui/ui.test.tsx` 10.
- Never in tests or the real window: NET (no GitHub: the `gh` seam answers from fixtures; never a signed-in gh), SECRET (tokens) (NET: no network; SECRET: secret values never in Debug, logs or failure text).
- Size (Hypothesis): read about 788 lines, write about 2047; split into 2: review mode + diff comments, then timeline + submit.
- Depends on: none.
- Note: Hypothesis: the five gh_pr_* commands have no webview caller (callers.py); settle with `git grep -n -E 'ghPr(Comment|List|Review|ReviewComments|View)' -- src` before planning them, they may belong to `[–]`.

### 26. `I-UNDO-PER-DOC` One undo history per document shared by its views (tab, code previews, Find previews)

- Webview sources (lines): `src/editor/core/model/document.ts` (919); spec files 233 lines.
- Native neighbours: `app-gpui/src/editor_model`, `app-gpui/src/editor/history.rs`, `app-gpui/src/ui/code_preview` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `unit:src/features/search/__tests__/codePreview.test.tsx`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/features/search/__tests__/codePreview.test.tsx` 9.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 1152 lines, write about 393; one writer.
- Depends on: none.
- Note: D134 'Not built: one undo history per document'. Find in Files, Replace and the code preview rows wait on it.

### 27. `F-SEARCH-A` Search: Find in Files and Replace (scope editor, resizing, previews), Find tool window

- Webview sources (lines): see the spec files; spec files 1282 lines.
- Native neighbours: `app-gpui/src/find`, `app-gpui/src/everywhere` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:find.inPath`, `act:find.replaceInPath`, `pt:search.tracking`, `pt:(at) src/features/search/find/parts.tsx:184`, `ovl:search.findInFiles`, `tw:find`, `evt:FsChanged`, `unit:src/features/search/__tests__/controller.test.ts`, `unit:src/features/search/__tests__/editorResults.test.tsx`, `unit:src/features/search/__tests__/findPopup.test.tsx`, `unit:src/features/search/__tests__/findWindow.test.ts`, `unit:src/features/search/__tests__/matchText.test.tsx`, `unit:src/features/search/__tests__/pathScopeActions.test.tsx`, `e2e:e2e/search.spec.ts`.
- Spec files and cases (total cases at HEAD; 14 still to prove): `e2e/search.spec.ts` 4, `src/features/search/__tests__/controller.test.ts` 11, `src/features/search/__tests__/editorResults.test.tsx` 1, `src/features/search/__tests__/findPopup.test.tsx` 26, `src/features/search/__tests__/findWindow.test.ts` 6, `src/features/search/__tests__/matchText.test.tsx` 3, `src/features/search/__tests__/pathScopeActions.test.tsx` 4.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 1282 lines, write about 522; one writer.
- Depends on: I-UNDO-PER-DOC (previews) (slices: I-UNDO-PER-DOC).

### 28. `F-RUN-SERVICES` Services window: configuration folders, running badge, group actions on folders (Run/Restart/Stop All, Open Browser)

- Webview sources (lines): `src/features/run/services.tsx` (953), `src/features/run/serviceOrder.ts` (80), `src/features/run/serviceSessions.ts` (97); spec files 1126 lines.
- Native neighbours: `app-gpui/src/run_testing/services.rs`, `app-gpui/src/workbench/services_window.rs`, `app-gpui/src/run_testing/service_sessions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:open-browser`, `act:restart-all`, `act:run-all`, `act:run.services.openBrowser`, `act:run.services.restartAll`, `act:run.services.runAll`, `act:run.services.stopAll`, `act:stop-all`, `boundid:toolwindow.services`, `ovl:run.serviceImport`, `unit:src/features/run/serviceOrder.test.ts`, `unit:src/features/run/services.test.ts`, `e2e:e2e/services.spec.ts`.
- Spec files and cases (total cases at HEAD; 19 still to prove): `e2e/services.spec.ts` 4, `src/features/run/serviceOrder.test.ts` 8, `src/features/run/services.test.ts` 46.
- Never in tests or the real window: DOCKER (Docker configuration with no connections, as since 7fedad68), TOOL (TOOL: no machine toolchain or real external program; DOCKER: never the Docker daemon).
- Size (Hypothesis): read about 2256 lines, write about 1157; one writer.
- Depends on: Q-FU2 (item 1).

### 29. `F-SET-FILE-TYPES` File types (override file type, revert), file colours, scopes page, caches local data folder, MCP server page, plugin formatters fold

- Webview sources (lines): `src/features/settings/pages/FileTypes.tsx` (159); spec files 131 lines.
- Native neighbours: `app-gpui/src/settings_view/pages` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:file.overrideFileType`, `act:file.revertFileTypeOverride`, `pt:settings.fileType`, `set:agents.chat`, `set:agents.mcpServer`, `set:fileColors`, `ipc:caches_local_data_info`, `ipc:caches_set_local_data_dir`, `ipc:mcp_regenerate_token`, `ipc:mcp_server_status`, `ipc:plugin_formatters`, `unit:src/features/settings/__tests__/agents.test.ts`, `unit:src/features/settings/__tests__/mcpServer.test.ts`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/features/settings/__tests__/agents.test.ts` 4, `src/features/settings/__tests__/mcpServer.test.ts` 5.
- Never in tests or the real window: HOME (the local data folder is the temp home's) (HOME: nothing of the user's home).
- Size (Hypothesis): read about 290 lines, write about 907; one writer.
- Depends on: none.

### 30. `F-RUN-TYPES-OTHER-A` Run types II: executors, targets, remote targets, sessions, runtimes, the type registry, navigation, stack traces

- Webview sources (lines): `src/features/run/executors.ts` (35), `src/features/run/navigate.ts` (190), `src/features/run/remote.ts` (186), `src/features/run/runtimes.ts` (165), `src/features/run/stackTrace.tsx` (167), `src/features/run/targets.ts` (551); spec files 3146 lines.
- Native neighbours: `app-gpui/src/run/types.rs`, `app-gpui/src/run/executors.rs`, `app-gpui/src/run/go_actions.rs`, `app-gpui/src/run/targets.rs`, `app-gpui/src/run/csharp_targets.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:analyze.stackTrace`, `act:debug`, `act:update`, `ctx:run.updatableRunning`, `pt:run.targets`, `pt:run.context`, `ovl:run.stackTraceDialog`, `pt:run.configs`, `pt:run`, `pt:runConfigurations`, `tw:run`, `feat:run`, `ipc:find_files_named`, `ipc:index_status`, `ipc:run_system_env`, `unit:src/features/run/executors.test.ts`, `unit:src/features/run/executorStop.test.ts`, `unit:src/features/run/navigate.test.ts`, `unit:src/features/run/remote.test.tsx`, `unit:src/features/run/runConfigTypes.test.tsx`, `unit:src/features/run/runtimes.test.ts`, `unit:src/features/run/session.test.tsx`, `unit:src/features/run/stackTrace.test.ts`, `unit:src/features/run/targets.test.ts`, `e2e:e2e/run.spec.ts`.
- Spec files and cases (total cases at HEAD; 74 still to prove): `e2e/run.spec.ts` 4, `src/features/run/executorStop.test.ts` 12, `src/features/run/executors.test.ts` 2, `src/features/run/navigate.test.ts` 5, `src/features/run/remote.test.tsx` 20, `src/features/run/runConfigTypes.test.tsx` 40, `src/features/run/runtimes.test.ts` 5, `src/features/run/session.test.tsx` 19, `src/features/run/stackTrace.test.ts` 4, `src/features/run/targets.test.ts` 26.
- Never in tests or the real window: TOOL (no dotnet, go, composer, bun, ruby: process seam), AGENT (agent runs: a recorded session only) (TOOL: no machine toolchain or real external program; AGENT: never start a coding agent).
- Size (Hypothesis): read about 4440 lines, write about 2714; split into 2: executors + targets + remote first, then the language types.
- Depends on: none.

### 31. `F-PL-NAV-EVENTS` Plugin navigation (index, markers, goto, files), workbench events to plugins (DocumentChanged, WebsocketReady, ProblemsChanged), endpoints, workspace contains, command backend, plugin text

- Webview sources (lines): `src/features/plugins/navigation.ts` (267), `src/features/plugins/runtimeUi.tsx` (762), `src/features/plugins/workspaceContains.ts` (115), `src/features/plugins/uiPoints.tsx` (609), `src/features/plugins/pluginText.ts` (164); spec files 802 lines.
- Native neighbours: `app-gpui/src/plugin_models/navigation.rs`, `app-gpui/src/workbench/plugin_runtime.rs`, `app-gpui/src/plugin_models/workspace_contains.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:(at) src/features/plugins/navigation.ts:170`, `pt:plugins.languageFeatures`, `pt:plugins.navigation`, `pt:(at) src/features/plugins/points.tsx:161`, `pt:(at) src/features/plugins/uiPoints.tsx:446`, `pt:plugins`, `pt:(at) src/features/plugins/uiPoints.tsx:185`, `pt:(at) src/features/plugins/navigation.ts:193`, `ipc:plugin_activation_event`, `ipc:plugin_endpoints`, `ipc:plugin_navigation_files`, `ipc:plugin_navigation_goto`, `ipc:plugin_navigation_index`, `ipc:plugin_navigation_invalidate`, `ipc:plugin_navigation_markers`, `ipc:plugin_post_events`, `ipc:plugin_run_command`, `ipc:plugin_set_ui_locale`, `evt:IndexState`, `unit:src/features/plugins/__tests__/commandBackend.test.ts`, `unit:src/features/plugins/__tests__/workspaceContains.test.ts`, `unit:src/features/plugins/pluginText.test.ts`, `e2e:e2e/pluginEndpoints.spec.ts`, `e2e:e2e/pluginFileTemplates.spec.ts`, `e2e:e2e/pluginMarkerIcons.spec.ts`.
- Spec files and cases (total cases at HEAD; 14 still to prove): `e2e/pluginEndpoints.spec.ts` 1, `e2e/pluginFileTemplates.spec.ts` 1, `e2e/pluginMarkerIcons.spec.ts` 1, `src/features/plugins/__tests__/commandBackend.test.ts` 5, `src/features/plugins/__tests__/workspaceContains.test.ts` 14, `src/features/plugins/pluginText.test.ts` 12.
- Never in tests or the real window: no plugin download; the api14 fixture is rebuilt only if the slice needs it (a change under crates/: the manager's call).
- Size (Hypothesis): read about 2719 lines, write about 1882; split into 2: navigation, then events + workspace contains.
- Depends on: T1-PLUGIN-VISUALS (slices: T1-PLUGIN-VISUALS).

### 32. `F-VCS-LOG` Git Log: ref labels, details batch, panes, loading, options, graph, branch memory, search, filter presets

- Webview sources (lines): `src/features/vcs/GitLog.tsx` (3129), `src/features/vcs/logPaneActions.ts` (226), `src/features/vcs/branchFilterPopup.tsx` (208), `src/features/vcs/graph.ts` (636), `src/features/vcs/logDetails.ts` (153), `src/features/vcs/logOptions.ts` (195), `src/features/vcs/logSearch.ts` (44); spec files 2708 lines.
- Native neighbours: `app-gpui/src/vcs/log_view.rs`, `app-gpui/src/vcs/log_details.rs`, `app-gpui/src/vcs_more/log_loading.rs`, `app-gpui/src/vcs/graph.rs`, `app-gpui/src/vcs_more/log_filters.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vcs.log.changes.compareBeforeWithLocal`, `act:vcs.log.changes.compareWithLocal`, `act:vcs.log.collapseLinear`, `act:vcs.log.compareVersions`, `act:vcs.log.compareWithLocal`, `act:vcs.log.expandLinear`, `act:vcs.log.goToRef`, `act:vcs.log.openExternal`, `act:vcs.log.pushUpToHere`, `act:vcs.log.squashCommits`, `act:vcs.showCommit`, `act:vcs.showLog`, `kind:git-log`, `set:vcs.log`, `ipc:vcs_commits_changes`, `ipc:vcs_log`, `ipc:vcs_log_details_batch`, `ipc:vcs_log_refs`, `ipc:vcs_revision_children`, `ipc:vcs_revision_files`, `unit:src/features/vcs/__tests__/branchFilterPopup.test.tsx`, `unit:src/features/vcs/__tests__/gitLogRefLabels.test.tsx`, `unit:src/features/vcs/__tests__/graph.test.ts`, `unit:src/features/vcs/__tests__/logBranchMemory.test.ts`, `unit:src/features/vcs/__tests__/logDetails.test.tsx`, `unit:src/features/vcs/__tests__/logFilters.test.ts`, `unit:src/features/vcs/__tests__/logGraphCorrectness.test.tsx`, `unit:src/features/vcs/__tests__/logLoading.test.ts`, `unit:src/features/vcs/__tests__/logOptions.test.ts`, `unit:src/features/vcs/__tests__/logPanes.test.tsx`, `unit:src/features/vcs/__tests__/logSearch.test.ts`, `unit:src/features/vcs/__tests__/logUiFeatures.test.tsx`, `unit:src/features/vcs/__tests__/logViews.test.tsx`, `e2e:e2e/gitLogBranchMenu.spec.ts`, `e2e:e2e/gitLogScrollbar.spec.ts`, `e2e:e2e/gitLogToolbarMenus.spec.ts`.
- Spec files and cases (total cases at HEAD; 47 still to prove): `e2e/gitLogBranchMenu.spec.ts` 1, `e2e/gitLogScrollbar.spec.ts` 4, `e2e/gitLogToolbarMenus.spec.ts` 1, `src/features/vcs/__tests__/branchFilterPopup.test.tsx` 5, `src/features/vcs/__tests__/gitLogRefLabels.test.tsx` 3, `src/features/vcs/__tests__/graph.test.ts` 12, `src/features/vcs/__tests__/logBranchMemory.test.ts` 10, `src/features/vcs/__tests__/logDetails.test.tsx` 4, `src/features/vcs/__tests__/logFilters.test.ts` 10, `src/features/vcs/__tests__/logGraphCorrectness.test.tsx` 11, `src/features/vcs/__tests__/logLoading.test.ts` 22, `src/features/vcs/__tests__/logOptions.test.ts` 17, `src/features/vcs/__tests__/logPanes.test.tsx` 15, `src/features/vcs/__tests__/logSearch.test.ts` 1, `src/features/vcs/__tests__/logUiFeatures.test.tsx` 11, `src/features/vcs/__tests__/logViews.test.tsx` 3.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 7299 lines, write about 3172; split into 3: loading/graph/options, then panes/details/labels.
- Depends on: Q-FU2, Q-FU4.

### 33. `F-EDITOR-TABS-SESSIONS-A` Editor tabs and sessions: tabs: pinned row, kept tabs, title revision, detach, layout actions, group panels

- Webview sources (lines): `src/editor/core/model` (2963), `src/editor/core/view` (2891); spec files 749 lines.
- Native neighbours: `app-gpui/src/editor/tabs.rs`, `app-gpui/src/editor/session.rs`, `app-gpui/src/workbench/light_edit_tabs.rs`, `app-gpui/src/editor/nav_history.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:editor.pinTab`, `act:editor.unsplit`, `act:editor.unsplitAll`, `mod:src/editor/core/model`, `mod:src/editor/core/view`, `unit:src/editor/core/detachTab.test.ts`, `unit:src/editor/core/view/editorLayoutActions.test.tsx`, `unit:src/editor/core/view/groupPanels.test.tsx`, `unit:src/editor/core/view/keptTabs.test.tsx`, `unit:src/editor/core/view/pinnedTabsRow.test.tsx`, `unit:src/editor/core/view/tabTitleRevision.test.tsx`, `unit:src/workbench/emptyEditorHints.test.tsx`.
- Spec files and cases (total cases at HEAD; 14 still to prove): `src/editor/core/detachTab.test.ts` 5, `src/editor/core/view/editorLayoutActions.test.tsx` 3, `src/editor/core/view/groupPanels.test.tsx` 2, `src/editor/core/view/keptTabs.test.tsx` 2, `src/editor/core/view/pinnedTabsRow.test.tsx` 4, `src/editor/core/view/tabTitleRevision.test.tsx` 3, `src/workbench/emptyEditorHints.test.tsx` 2.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 6603 lines, write about 694; one writer.
- Depends on: none.

### 34. `F-HTTP` HTTP Client: environment files (add public/private), Copy as cURL and paste conversion, file and variable navigation, headers, response find, endpoints tab, Spring

- Webview sources (lines): `src/features/http-client` (3427), `src/features/http-client/endpoints.ts` (176), `src/features/http-client/fileNavigation.ts` (44), `src/features/http-client/headers.ts` (131), `src/features/http-client/variableNavigation.ts` (54); spec files 829 lines.
- Native neighbours: `app-gpui/src/http_client`, `app-gpui/src/features_a/http_client`, `app-gpui/src/workbench/http_client` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:http.addPrivateEnvironment`, `act:http.addPublicEnvironment`, `act:http.copyAsCurl`, `act:http.newRequest`, `pt:http.endpoints`, `pt:http.requests`, `pt:http.spring`, `pt:(at) src/features/http-client/index.ts:142`, `pt:http.new`, `pt:(at) src/features/http-client/fileNavigation.ts:29`, `pt:(at) src/features/http-client/variableNavigation.ts:39`, `pt:httpClient`, `feat:http-client`, `ipc:http_curl`, `ipc:http_environment_file`, `ipc:http_from_curl`, `ipc:http_variable_definitions`, `unit:src/features/http-client/endpoints.test.ts`, `unit:src/features/http-client/fileNavigation.test.ts`, `unit:src/features/http-client/headers.test.ts`, `unit:src/features/http-client/httpClient.test.ts`, `unit:src/features/http-client/responseFind.test.tsx`, `unit:src/features/http-client/spring/spring.test.ts`, `unit:src/features/http-client/variableNavigation.test.ts`.
- Spec files and cases (total cases at HEAD; 13 still to prove): `src/features/http-client/endpoints.test.ts` 3, `src/features/http-client/fileNavigation.test.ts` 2, `src/features/http-client/headers.test.ts` 3, `src/features/http-client/httpClient.test.ts` 19, `src/features/http-client/responseFind.test.tsx` 1, `src/features/http-client/spring/spring.test.ts` 20, `src/features/http-client/variableNavigation.test.ts` 2.
- Never in tests or the real window: NET (requests only to a loopback listener the test opens) (NET: no network).
- Size (Hypothesis): read about 4661 lines, write about 1567; split into 2 by the native neighbours it touches.
- Depends on: Q-VIS2 (header slot), Q-FU4.

### 35. `I-ACTIONS-ENABLED` Actions: `enabled(ctx, arg)` predicate, `indicatesRunning`, batched registration

- Webview sources (lines): `src/platform/actions.ts` (260); spec files 120 lines.
- Native neighbours: `app-gpui/src/platform/actions.rs`, `app-gpui/src/platform/dispatcher.rs`, `app-gpui/src/workbench/menu_spec.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:edit.selectAll`, `act:edit.undo`, `mod:src/platform/actions.ts`, `unit:src/platform/actions.test.ts`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/platform/actions.test.ts` 5.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 380 lines, write about 253; one writer.
- Depends on: Q-FU4 (its keymap flag lands first).
- Note: 46 webview actions use `enabled`; menus, toolbars and Find Action must draw them disabled. Needed by every slice whose actions have an `enabled` (named per slice).

### 36. `F-PLAT-MODELS-A` Platform models I: lossless JSON, scopes, double click, pointer focus, navigation modifier, keymap mouse shortcuts, IDEA keymap import

- Webview sources (lines): `src/platform/doubleClick.ts` (55), `src/platform/ideaActions.ts` (102), `src/platform/ideaKeymap.ts` (160), `src/platform/keymap.ts` (772), `src/platform/keymaps/vscode-macos.ts` (435), `src/platform/keymaps/vscode-windows.ts` (71), `src/platform/losslessJson.ts` (214), `src/platform/navigationModifier.ts` (78), `src/platform/pointerFocus.ts` (34), `src/platform/scopes.ts` (255); spec files 1372 lines.
- Native neighbours: `app-gpui/src/platform/keymap.rs`, `app-gpui/src/platform/dispatcher.rs`, `app-gpui/src/platform/input.rs`, `app-gpui/src/find/scopes_dialog.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:search.editScopes`, `ovl:search.scopes`, `set:scopes`, `mod:src/platform/doubleClick.ts`, `mod:src/platform/ideaActions.ts`, `mod:src/platform/ideaKeymap.ts`, `mod:src/platform/keymap.ts`, `mod:src/platform/keymaps/vscode-macos.ts`, `mod:src/platform/keymaps/vscode-windows.ts`, `mod:src/platform/losslessJson.ts`, `mod:src/platform/navigationModifier.ts`, `mod:src/platform/pointerFocus.ts`, `mod:src/platform/scopes.ts`, `unit:src/features/settings/__tests__/mouseShortcut.test.tsx`, `unit:src/platform/doubleClick.test.ts`, `unit:src/platform/ideaKeymap.test.ts`, `unit:src/platform/keymap.test.ts`, `unit:src/platform/keymapMouse.test.ts`, `unit:src/platform/keymaps/eclipse.test.ts`, `unit:src/platform/losslessJson.test.ts`, `unit:src/platform/navigationModifier.test.ts`, `unit:src/platform/pointerFocus.test.ts`, `unit:src/platform/scopes.test.ts`.
- Spec files and cases (total cases at HEAD; 57 still to prove): `src/features/settings/__tests__/mouseShortcut.test.tsx` 3, `src/platform/doubleClick.test.ts` 5, `src/platform/ideaKeymap.test.ts` 4, `src/platform/keymap.test.ts` 22, `src/platform/keymapMouse.test.ts` 13, `src/platform/keymaps/eclipse.test.ts` 9, `src/platform/losslessJson.test.ts` 8, `src/platform/navigationModifier.test.ts` 1, `src/platform/pointerFocus.test.ts` 3, `src/platform/scopes.test.ts` 9.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 3548 lines, write about 2627; split into 2 by the native neighbours it touches.
- Depends on: Q-FU4 (dispatcher keyWhen rule).

### 37. `F-LSP-INLINE-B` Inline completion, inlay hints and requests, code lens (server and author), interactive commands, format errors, spelling: code lens (server, author), interactive commands, format, spelling

- Webview sources (lines): `src/editor/lsp/authorLens.ts` (102), `src/editor/lsp/codeLens.ts` (363), `src/editor/lsp/format.ts` (297), `src/editor/lsp/interactiveCommand.ts` (84), `src/editor/lsp/serverCodeLens.ts` (101), `src/editor/lsp/spelling.ts` (271), `src/editor/core/cm/codeLens.ts` (285); spec files 1182 lines.
- Native neighbours: `app-gpui/src/editor_lsp/inline_completion.rs`, `app-gpui/src/editor_lsp/inlay.rs`, `app-gpui/src/editor_lsp/code_lens.rs`, `app-gpui/src/editor_lsp/spelling.rs`, `app-gpui/src/editor_lsp/format.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:code.cleanupFile`, `act:code.reformat`, `act:code.reformatDialog`, `act:code.reformatFile`, `pt:(at) src/editor/lsp/actions.ts:159`, `pt:lsp.formatOnSave`, `mod:src/editor/lsp/authorLens.ts`, `mod:src/editor/lsp/codeLens.ts`, `mod:src/editor/lsp/format.ts`, `mod:src/editor/lsp/interactiveCommand.ts`, `mod:src/editor/lsp/serverCodeLens.ts`, `mod:src/editor/lsp/spelling.ts`, `ipc:lsp_code_lens`, `ipc:lsp_resolve_command`, `ipc:spell_check`, `ipc:spell_suggest`, `unit:src/editor/core/cm/codeLens.test.ts`, `unit:src/editor/lsp/authorLens.test.ts`, `unit:src/editor/lsp/codeLens.test.ts`, `unit:src/editor/lsp/formatErrors.test.tsx`, `unit:src/editor/lsp/interactiveCommand.test.tsx`, `unit:src/editor/lsp/serverCodeLens.test.ts`, `unit:src/editor/lsp/spelling.test.ts`.
- Spec files and cases (total cases at HEAD; 39 still to prove): `src/editor/core/cm/codeLens.test.ts` 7, `src/editor/lsp/authorLens.test.ts` 5, `src/editor/lsp/codeLens.test.ts` 23, `src/editor/lsp/formatErrors.test.tsx` 5, `src/editor/lsp/interactiveCommand.test.tsx` 7, `src/editor/lsp/serverCodeLens.test.ts` 4, `src/editor/lsp/spelling.test.ts` 7.
- Never in tests or the real window: TOOL (stand-in language server; no real spell checker dictionary download) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 2685 lines, write about 2038; split into 2: inline completion + inlay, then lenses + format + spelling.
- Depends on: none.

### 38. `F-VCS-HISTORY` File history, revisions, annotate a revision, blame, Show Diff for Lines, local history, recent changes

- Webview sources (lines): `src/features/vcs/fileRevisions.ts` (86), `src/features/vcs/FileRevisionChooser.tsx` (173), `src/features/vcs/annotateRevision.ts` (95), `src/features/vcs/blame.ts` (364), `src/features/vcs/LocalHistory.tsx` (286), `src/features/vcs/SelectionHistory.tsx` (154), `src/features/vcs/fileRevisionChooser.tsx` (173), `src/features/vcs/inlineBlame.ts` (461), `src/features/vcs/localHistory.tsx` (286), `src/features/vcs/recentChanges.ts` (59), `src/features/vcs/selectionHistory.tsx` (154), `src/features/vcs/diffBlame.ts` (27); spec files 1316 lines.
- Native neighbours: `app-gpui/src/vcs/revision_chooser.rs`, `app-gpui/src/vcs/local_history_view.rs`, `app-gpui/src/vcs/selection_history_view.rs`, `app-gpui/src/workbench/annotate.rs`, `app-gpui/src/vcs_diff/inline_blame.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:localHistory.recentChanges`, `act:merge.annotate`, `act:vcs.compareWithRevision`, `act:vcs.shelf.showDiffNewTab`, `act:vcs.showCurrentRevision`, `act:vcs.showDiffForLines`, `act:vcs.showHistory`, `act:vcs.showHistoryForSelection`, `ctx:vcsFileHistoryVersion`, `kind:git-revision`, `kind:localHistory`, `kind:selectionHistory`, `ipc:vcs_ignore_blame_revision`, `unit:src/features/vcs/__tests__/annotateRevision.test.ts`, `unit:src/features/vcs/__tests__/blame.test.tsx`, `unit:src/features/vcs/__tests__/fileRevisionChooser.test.tsx`, `unit:src/features/vcs/__tests__/fileRevisions.test.tsx`, `unit:src/features/vcs/__tests__/inlineBlame.test.ts`, `unit:src/features/vcs/__tests__/localHistory.test.tsx`, `unit:src/features/vcs/__tests__/recentChanges.test.ts`, `unit:src/features/vcs/__tests__/selectionHistory.test.tsx`, `unit:src/features/vcs/__tests__/showDiffForLines.test.ts`, `unit:src/features/vcs/diffBlame.test.ts`.
- Spec files and cases (total cases at HEAD; 33 still to prove): `src/features/vcs/__tests__/annotateRevision.test.ts` 6, `src/features/vcs/__tests__/blame.test.tsx` 1, `src/features/vcs/__tests__/fileRevisionChooser.test.tsx` 3, `src/features/vcs/__tests__/fileRevisions.test.tsx` 8, `src/features/vcs/__tests__/inlineBlame.test.ts` 20, `src/features/vcs/__tests__/localHistory.test.tsx` 5, `src/features/vcs/__tests__/recentChanges.test.ts` 2, `src/features/vcs/__tests__/selectionHistory.test.tsx` 6, `src/features/vcs/__tests__/showDiffForLines.test.ts` 2, `src/features/vcs/diffBlame.test.ts` 3.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 3634 lines, write about 2305; split into 2: history and revisions, then blame/annotate.
- Depends on: none (slices: I-ACTIONS-ENABLED).

### 39. `F-LSP-POPUPS-C` Completion, hover, docs, signature, intentions, quick fixes, type info, hint, Popups, auto import and imports on paste, in-place rename, outside-project files: intentions, quick fixes, rename in place, imports, outside project, availability, diagnostics

- Webview sources (lines): `src/editor/lsp/actions.ts` (378), `src/editor/lsp/autoImport.ts` (128), `src/editor/lsp/availability.ts` (164), `src/editor/lsp/diagnostics.ts` (360), `src/editor/lsp/files.ts` (86), `src/editor/lsp/generate.ts` (9), `src/editor/lsp/importsOnPaste.ts` (401), `src/editor/lsp/index.ts` (150), `src/editor/lsp/inplaceRename.ts` (255), `src/editor/lsp/inspectionWidget.ts` (167), `src/editor/lsp/intentions.ts` (784), `src/editor/lsp/lsp.ts` (109), `src/editor/lsp/outsideProject.ts` (95), `src/editor/lsp/pluginVisuals.ts` (16), `src/editor/lsp/rename.ts` (424), `src/editor/lsp/workspaceEdit.ts` (145); spec files 1734 lines.
- Native neighbours: `app-gpui/src/editor_lsp/completion.rs`, `app-gpui/src/editor_lsp/hover.rs`, `app-gpui/src/editor_lsp/signature.rs`, `app-gpui/src/editor_lsp/intentions.rs`, `app-gpui/src/editor_lsp/inplace_rename.rs`, `app-gpui/src/editor_lsp/outside_project.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:code.applyPastedImports`, `act:code.fixEslintProblems`, `act:code.showIntentions`, `act:refactor.rename`, `pt:lsp.outsideProject`, `pt:lsp`, `pt:lsp.lineMarkers`, `pt:lsp.parameterTab`, `mod:src/editor/lsp/actions.ts`, `mod:src/editor/lsp/autoImport.ts`, `mod:src/editor/lsp/availability.ts`, `mod:src/editor/lsp/diagnostics.ts`, `mod:src/editor/lsp/files.ts`, `mod:src/editor/lsp/generate.ts`, `mod:src/editor/lsp/importsOnPaste.ts`, `mod:src/editor/lsp/index.ts`, `mod:src/editor/lsp/inplaceRename.ts`, `mod:src/editor/lsp/inspectionWidget.ts`, `mod:src/editor/lsp/intentions.ts`, `mod:src/editor/lsp/lsp.ts`, `mod:src/editor/lsp/outsideProject.ts`, `mod:src/editor/lsp/pluginVisuals.ts`, `mod:src/editor/lsp/rename.ts`, `mod:src/editor/lsp/workspaceEdit.ts`, `evt:DocLanguage`, `evt:LanguageServerWarning`, `evt:LspRefresh`, `unit:src/editor/lsp/availability.test.ts`, `unit:src/editor/lsp/diagnostics.test.ts`, `unit:src/editor/lsp/features.test.ts`, `unit:src/editor/lsp/inplaceRename.test.ts`, `unit:src/editor/lsp/intentions.test.ts`, `unit:src/editor/lsp/outsideProject.test.ts`, `e2e:e2e/codeInsight.spec.ts`.
- Spec files and cases (total cases at HEAD; 28 still to prove): `e2e/codeInsight.spec.ts` 10, `src/editor/lsp/availability.test.ts` 9, `src/editor/lsp/diagnostics.test.ts` 5, `src/editor/lsp/features.test.ts` 21, `src/editor/lsp/inplaceRename.test.ts` 8, `src/editor/lsp/intentions.test.ts` 23, `src/editor/lsp/outsideProject.test.ts` 3.
- Never in tests or the real window: TOOL (stand-in language server) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 5405 lines, write about 3348; split into 3: completion (55 cases) alone; then hover/docs/signature; then intentions/rename/imports.
- Depends on: Q-FU3, Q-FU4.

### 40. `F-RUN-TESTS` Test runner: tree view, rerun failed, expand/collapse, comparison, plugin/dotnet/Zig/Ruby tests, test reports

- Webview sources (lines): `src/features/run/TestTreeView.tsx` (376), `src/features/run/testing` (2782), `src/features/run/testTree.ts` (8), `src/features/run/junitType.ts` (161), `src/features/run/rubyTests.ts` (47), `src/features/run/testing/comparison.ts` (56), `src/features/run/testing/dotnet.ts` (95), `src/features/run/testing/plugin.ts` (146), `src/features/run/testing/rerun.ts` (294), `src/features/run/testing/states.ts` (114), `src/features/run/testing/zig.ts` (149); spec files 2145 lines.
- Native neighbours: `app-gpui/src/run_testing`, `app-gpui/src/run/view.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:run.collapseAllTests`, `act:run.expandAllTests`, `act:run.rerunFailed`, `ctx:run.canRerunFailed`, `pt:dotnet-test`, `pt:junit`, `pt:phpunit`, `pt:rspec`, `ipc:plugin_parse_test_results`, `ipc:plugin_select_tests`, `ipc:run_test_reports`, `unit:src/features/run/junitType.test.ts`, `unit:src/features/run/rubyTests.test.ts`, `unit:src/features/run/testing/comparison.test.ts`, `unit:src/features/run/testing/dotnet.test.ts`, `unit:src/features/run/testing/parsers.test.ts`, `unit:src/features/run/testing/plugin.test.ts`, `unit:src/features/run/testing/rerun.test.ts`, `unit:src/features/run/testing/states.test.ts`, `unit:src/features/run/testing/zig.test.ts`, `unit:src/features/run/TestTreeView.test.tsx`, `e2e:e2e/testRunner-ui.spec.ts`.
- Spec files and cases (total cases at HEAD; 36 still to prove): `e2e/testRunner-ui.spec.ts` 1, `src/features/run/TestTreeView.test.tsx` 11, `src/features/run/junitType.test.ts` 3, `src/features/run/rubyTests.test.ts` 4, `src/features/run/testing/comparison.test.ts` 7, `src/features/run/testing/dotnet.test.ts` 8, `src/features/run/testing/parsers.test.ts` 34, `src/features/run/testing/plugin.test.ts` 3, `src/features/run/testing/rerun.test.ts` 26, `src/features/run/testing/states.test.ts` 6, `src/features/run/testing/zig.test.ts` 8.
- Never in tests or the real window: TOOL (recorded test output, no test framework runs) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 6373 lines, write about 2133; split into 2 by the native neighbours it touches.
- Depends on: none.

### 41. `F-PL-DETAILS` Plugins page: Requires / Required by / Extension points / net hosts sections, revoke a net host, ask before disabling a needed plugin, install from disk, IntelliJ themes

- Webview sources (lines): `src/features/plugins/PluginsPage.tsx` (1531), `src/features/plugins/intellijImport.ts` (41), `src/features/plugins/market.ts` (85), `src/features/plugins/settings.tsx` (395); spec files 3118 lines.
- Native neighbours: `app-gpui/src/plugin_models/plugins_page.rs`, `app-gpui/src/plugin_views` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:plugins.importInstalledIntellijTheme`, `act:plugins.importIntellijTheme`, `act:plugins.installFromDisk`, `ovl:plugins.intellijThemes`, `set:(at) src/features/plugins/settings.tsx:376`, `set:(at) src/features/plugins/settings.tsx:386`, `set:plugins`, `ipc:plugin_installed_intellij_themes`, `ipc:plugin_list`, `ipc:plugin_read_intellij_theme`, `ipc:plugin_revoke_net_host`, `ipc:plugin_set_enabled`, `ipc:plugin_set_theme_frame`, `ipc:plugin_stage_package`, `unit:src/features/plugins/__tests__/detailViews.test.tsx`, `unit:src/features/plugins/__tests__/intellijImport.test.tsx`, `unit:src/features/plugins/__tests__/market.test.ts`, `unit:src/features/plugins/__tests__/plugins.test.tsx`, `unit:src/features/plugins/__tests__/settings.test.tsx`, `unit:src/features/plugins/__tests__/startupTheme.test.ts`, `e2e:e2e/plugin-settings.spec.ts`, `e2e:e2e/pluginDetails.spec.ts`.
- Spec files and cases (total cases at HEAD; 72 still to prove): `e2e/plugin-settings.spec.ts` 1, `e2e/pluginDetails.spec.ts` 1, `src/features/plugins/__tests__/detailViews.test.tsx` 12, `src/features/plugins/__tests__/intellijImport.test.tsx` 6, `src/features/plugins/__tests__/market.test.ts` 5, `src/features/plugins/__tests__/plugins.test.tsx` 78, `src/features/plugins/__tests__/settings.test.tsx` 14, `src/features/plugins/__tests__/startupTheme.test.ts` 5.
- Never in tests or the real window: NET (the marketplace is never fetched; an index file in the test), HOME (themes read from a temp folder) (NET: no network; HOME: nothing of the user's home).
- Size (Hypothesis): read about 5170 lines, write about 2854; split into 2 by the native neighbours it touches.
- Depends on: Q-FU3.

### 42. `F-RUN-TYPES-OTHER-B` Run types II: language types: .NET, Go, PHP Composer, npm, Bun, Ruby/Rake, Unity Editor, launch settings

- Webview sources (lines): `src/features/run/composerInstall.ts` (149), `src/features/run/dotnetTypes.ts` (199), `src/features/run/goActions.ts` (112), `src/features/run/launchSettingsGutter.ts` (103); spec files 329 lines.
- Native neighbours: `app-gpui/src/run/types.rs`, `app-gpui/src/run/executors.rs`, `app-gpui/src/run/go_actions.rs`, `app-gpui/src/run/targets.rs`, `app-gpui/src/run/csharp_targets.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:go.generate.file`, `act:go.generate.package`, `act:php.composerInstall`, `pt:php.composerInstall`, `pt:run.launchSettings`, `pt:run.npmScripts`, `pt:go.modules`, `pt:bun`, `pt:dotnet`, `pt:npm`, `pt:rake`, `pt:ruby`, `pt:unity-editor`, `ipc:sdk_detect`, `unit:src/features/run/composerInstall.test.tsx`, `unit:src/features/run/dotnetTypes.test.tsx`, `unit:src/features/run/goActions.test.ts`, `unit:src/features/run/launchSettingsGutter.test.ts`.
- Spec files and cases (total cases at HEAD; 10 still to prove): `src/features/run/composerInstall.test.tsx` 2, `src/features/run/dotnetTypes.test.tsx` 5, `src/features/run/goActions.test.ts` 3, `src/features/run/launchSettingsGutter.test.ts` 2.
- Never in tests or the real window: TOOL (no dotnet, go, composer, bun, ruby: process seam), AGENT (agent runs: a recorded session only) (TOOL: no machine toolchain or real external program; AGENT: never start a coding agent).
- Size (Hypothesis): read about 892 lines, write about 1337; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 83 %; Hypothesis).
- Depends on: none.

### 43. `F-TERMINAL` Terminal: find bar, move to editor, CLI sessions and kinds, links and highlights, plugin sessions, close last tab, embedded fit, IME, paste, image store

- Webview sources (lines): `src/features/terminal/` (5240), `src/features/terminal/cli.ts` (76), `src/features/terminal/cliSessions.ts` (99), `src/features/terminal/domRowCache.ts` (153), `src/features/terminal/drop.ts` (48), `src/features/terminal/embedded.ts` (158), `src/features/terminal/highlight.ts` (593), `src/features/terminal/imeCommit.ts` (136), `src/features/terminal/imeInput.ts` (155), `src/features/terminal/linkHighlights.ts` (131), `src/features/terminal/paste.ts` (171), `src/features/terminal/sampleTui.ts` (42), `src/features/terminal/session.ts` (901), `src/features/terminal/windowsPty.ts` (21); spec files 4098 lines.
- Native neighbours: `app-gpui/src/terminal`, `app-gpui/src/workbench/terminal_groups.rs`, `app-gpui/src/workbench/terminal_actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:terminal.find`, `act:terminal.moveToEditor`, `act:terminal.pluginSession`, `act:terminal.sampleTui`, `act:terminal.splitAndMoveDown`, `pt:terminal`, `kind:cli`, `pt:(at) src/features/terminal/drop.ts:23`, `pt:(at) src/features/terminal/session.ts:382`, `tw:terminal`, `feat:terminal`, `ipc:term_store_image`, `unit:src/features/terminal/cli.test.ts`, `unit:src/features/terminal/cliKinds.test.tsx`, `unit:src/features/terminal/cliSessions.test.ts`, `unit:src/features/terminal/closeLastTab.test.ts`, `unit:src/features/terminal/domRowCache.test.ts`, `unit:src/features/terminal/drop.test.ts`, `unit:src/features/terminal/embedded.test.ts`, `unit:src/features/terminal/embeddedFit.test.ts`, `unit:src/features/terminal/fontLoading.test.ts`, `unit:src/features/terminal/highlight.test.ts`, `unit:src/features/terminal/imeCommit.test.ts`, `unit:src/features/terminal/imeInput.test.ts`, `unit:src/features/terminal/linkHighlights.test.ts`, `unit:src/features/terminal/paste.test.ts`, `unit:src/features/terminal/sampleTui.test.ts`, `unit:src/features/terminal/session.test.ts`, `unit:src/features/terminal/terminal.test.ts`, `unit:src/features/terminal/windowsPty.test.ts`, `e2e:e2e/terminal-clipboard.spec.ts`, `e2e:e2e/terminal-ui.spec.ts`.
- Spec files and cases (total cases at HEAD; 90 still to prove): `e2e/terminal-clipboard.spec.ts` 1, `e2e/terminal-ui.spec.ts` 4, `src/features/terminal/cli.test.ts` 8, `src/features/terminal/cliKinds.test.tsx` 6, `src/features/terminal/cliSessions.test.ts` 8, `src/features/terminal/closeLastTab.test.ts` 5, `src/features/terminal/domRowCache.test.ts` 5, `src/features/terminal/drop.test.ts` 6, `src/features/terminal/embedded.test.ts` 4, `src/features/terminal/embeddedFit.test.ts` 2, `src/features/terminal/fontLoading.test.ts` 1, `src/features/terminal/highlight.test.ts` 8, `src/features/terminal/imeCommit.test.ts` 15, `src/features/terminal/imeInput.test.ts` 23, `src/features/terminal/linkHighlights.test.ts` 8, `src/features/terminal/paste.test.ts` 25, `src/features/terminal/sampleTui.test.ts` 1, `src/features/terminal/session.test.ts` 23, `src/features/terminal/terminal.test.ts` 43, `src/features/terminal/windowsPty.test.ts` 1.
- Never in tests or the real window: TOOL (never a login or interactive shell: the test build refuses; a stand-in program), HOME (temp HOME and ZDOTDIR) (TOOL: no machine toolchain or real external program; HOME: nothing of the user's home).
- Size (Hypothesis): read about 12022 lines, write about 3721; split into 3: find bar + links + highlight, then CLI sessions, then IME/paste.
- Depends on: none.

### 44. `F-EDITOR-TABS-SESSIONS-B` Editor tabs and sessions: sessions: busy and session close, input close, file-open order, light edit, navigation history

- Webview sources (lines): `src/editor/core/controller.ts` (2340), `src/editor/core/index.ts` (151), `src/editor/core/model/workspaceHistory.ts` (177); spec files 2310 lines.
- Native neighbours: `app-gpui/src/editor/tabs.rs`, `app-gpui/src/editor/session.rs`, `app-gpui/src/workbench/light_edit_tabs.rs`, `app-gpui/src/editor/nav_history.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:file.toggleReadOnly`, `act:find.openInWindow`, `act:navigate.back`, `act:navigate.forward`, `stat:app.lightEdit`, `mod:src/editor/core/controller.ts`, `mod:src/editor/core/index.ts`, `ipc:doc_save_as`, `ipc:doc_ui_mode`, `ipc:window_open_file_in_new_window`, `unit:src/editor/core/busyClose.test.ts`, `unit:src/editor/core/controller.test.ts`, `unit:src/editor/core/file-open-order.test.ts`, `unit:src/editor/core/inputClose.test.ts`, `unit:src/editor/core/lightedit-launch.test.ts`, `unit:src/editor/core/model/document-open-latency.test.ts`, `unit:src/editor/core/model/model.test.ts`, `unit:src/editor/core/model/workspaceHistory.test.ts`, `unit:src/editor/core/session-close.test.ts`, `unit:src/workbench/externalFiles.test.ts`, `unit:src/workbench/lightEdit.test.tsx`.
- Spec files and cases (total cases at HEAD; 53 still to prove): `src/editor/core/busyClose.test.ts` 7, `src/editor/core/controller.test.ts` 18, `src/editor/core/file-open-order.test.ts` 15, `src/editor/core/inputClose.test.ts` 1, `src/editor/core/lightedit-launch.test.ts` 10, `src/editor/core/model/document-open-latency.test.ts` 4, `src/editor/core/model/model.test.ts` 21, `src/editor/core/model/workspaceHistory.test.ts` 11, `src/editor/core/session-close.test.ts` 3, `src/workbench/externalFiles.test.ts` 5, `src/workbench/lightEdit.test.tsx` 7.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 4978 lines, write about 2427; split into 2: tabs, then sessions and close.
- Depends on: none.

### 45. `F-EDITOR-DOCS-SYNC-B` Documents: library sources, file attributes, encoding and BOM, banners, status items, copy reference

- Webview sources (lines): `src/editor/core/colorSchemeRuntime.ts` (80), `src/editor/core/copyReference.ts` (115), `src/editor/core/fileAttributes.ts` (63), `src/editor/core/librarySources.ts` (149), `src/editor/core/view/banner.tsx` (132), `src/editor/core/view/statusItems.tsx` (525); spec files 896 lines.
- Native neighbours: `app-gpui/src/editor_model`, `app-gpui/src/editor/buffer.rs`, `app-gpui/src/platform/virtual_docs.rs`, `app-gpui/src/workbench/status_items.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:editor.cancelSourceDownload`, `act:editor.chooseSources`, `act:editor.copyPathReference`, `act:editor.downloadSources`, `act:editor.encodings.toggleNativeToAscii`, `ctx:findBarHasResults`, `stat:editor.indent`, `mod:src/editor/core/colorSchemeRuntime.ts`, `mod:src/editor/core/copyReference.ts`, `mod:src/editor/core/fileAttributes.ts`, `mod:src/editor/core/librarySources.ts`, `ipc:doc_set_bom`, `ipc:lsp_can_download_sources`, `ipc:lsp_library_sources`, `ipc:lsp_resolve_class_file`, `unit:src/editor/core/copyPathPopup.test.tsx`, `unit:src/editor/core/fileAttributes.test.ts`, `unit:src/editor/core/librarySources.test.tsx`, `unit:src/editor/core/view/banner.test.tsx`, `unit:src/editor/core/view/encodingStatus.test.tsx`, `unit:src/editor/core/view/statusItems.test.tsx`.
- Spec files and cases (total cases at HEAD; 27 still to prove): `src/editor/core/copyPathPopup.test.tsx` 11, `src/editor/core/fileAttributes.test.ts` 5, `src/editor/core/librarySources.test.tsx` 6, `src/editor/core/view/banner.test.tsx` 2, `src/editor/core/view/encodingStatus.test.tsx` 4, `src/editor/core/view/statusItems.test.tsx` 9.
- Never in tests or the real window: NET (Download Sources: a stand-in; no Maven Central) (NET: no network).
- Size (Hypothesis): read about 1960 lines, write about 1848; split into 2: sync and virtual docs, then library sources.
- Depends on: Q-FU3 (encoding/BOM).

### 46. `F-AG-COMPOSER-TASKS-C` Agent composer and tasks: changes panel, home editor, AgentsView, unread, working status, policy, status item

- Webview sources (lines): `src/features/agents/AgentsView.tsx` (519), `src/features/agents/HomeEditor.tsx` (345), `src/features/agents/fontZoom.ts` (95), `src/features/agents/loginHint.ts` (9), `src/features/agents/workingStatusPage.tsx` (147); spec files 2396 lines.
- Native neighbours: `app-gpui/src/agents/composer.rs`, `app-gpui/src/agents/task_list.rs`, `app-gpui/src/agents/new_task_view.rs`, `app-gpui/src/agents/task_actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:changes`, `stat:agents.status`, `set:agents.policy`, `set:agents.workingStatus`, `ipc:agent_accept_changes`, `ipc:agent_base_text`, `ipc:agent_changes`, `ipc:agent_commands`, `ipc:agent_create`, `ipc:agent_grouped_changes`, `ipc:agent_probe`, `ipc:agent_rollback_changes`, `evt:Task`, `unit:src/features/agents/agentPolicy.test.tsx`, `unit:src/features/agents/agents.test.ts`, `unit:src/features/agents/AgentsView.test.tsx`, `unit:src/features/agents/fontZoom.test.tsx`, `unit:src/features/agents/HomeEditor.test.tsx`, `unit:src/features/agents/loginHint.test.ts`, `unit:src/features/agents/unread.test.tsx`, `unit:src/features/agents/workingStatusPage.test.tsx`.
- Spec files and cases (total cases at HEAD; 54 still to prove): `src/features/agents/AgentsView.test.tsx` 31, `src/features/agents/HomeEditor.test.tsx` 9, `src/features/agents/agentPolicy.test.tsx` 2, `src/features/agents/agents.test.ts` 29, `src/features/agents/fontZoom.test.tsx` 5, `src/features/agents/loginHint.test.ts` 4, `src/features/agents/unread.test.tsx` 23, `src/features/agents/workingStatusPage.test.tsx` 4.
- Never in tests or the real window: AGENT (fake agent only; spawn_guard), HOME (external sessions are read from a temp folder only) (AGENT: never start a coding agent; HOME: nothing of the user's home).
- Size (Hypothesis): read about 3511 lines, write about 2232; split into 2: composer (folders 29, submission 17, commands), then task groups and organize, then changes panel.
- Depends on: Q-FU2 (item 4).

### 47. `F-DB-CONSOLES` Several query consoles per data source, New Query Console, Jump to Console list, SQL script, MyBatis, SQL resolution scopes, Snowflake commands and highlighting

- Webview sources (lines): `src/features/database/console.tsx` (1024), `src/features/database/mybatis.ts` (123), `src/features/database/resolutionScopes.ts` (61), `src/features/database/sql` (2984), `src/features/database/SqlHighlight.tsx` (49), `src/features/database/sql/format.ts` (264), `src/features/database/sql/mapper.ts` (154), `src/features/database/sql/readOnly.ts` (24), `src/features/database/sql/snowflakeHighlight.ts` (73); spec files 1351 lines.
- Native neighbours: `app-gpui/src/database/console_view.rs`, `app-gpui/src/database/sql`, `app-gpui/src/workbench/database_extras.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:db.executeScript`, `act:db.jumpToConsole`, `act:db.new`, `act:db.newConsole`, `act:db.openConsole`, `act:db.quickDoc`, `act:db.runSqlScript`, `act:db.sqlScript`, `pt:database.console`, `pt:database.mybatis`, `set:languages.sqlResolutionScopes`, `ipc:db_consoles`, `unit:src/features/database/mybatis.test.ts`, `unit:src/features/database/resolutionScopes.test.ts`, `unit:src/features/database/sql/format.test.ts`, `unit:src/features/database/sql/mapper.test.ts`, `unit:src/features/database/sql/readOnly.test.ts`, `unit:src/features/database/sql/snowflakeCommands.test.ts`, `unit:src/features/database/sql/snowflakeHighlight.test.ts`, `unit:src/features/database/sql/sql.test.ts`, `unit:src/features/database/SqlHighlight.test.tsx`.
- Spec files and cases (total cases at HEAD; 30 still to prove): `src/features/database/SqlHighlight.test.tsx` 2, `src/features/database/mybatis.test.ts` 4, `src/features/database/resolutionScopes.test.ts` 3, `src/features/database/sql/format.test.ts` 5, `src/features/database/sql/mapper.test.ts` 3, `src/features/database/sql/readOnly.test.ts` 3, `src/features/database/sql/snowflakeCommands.test.ts` 5, `src/features/database/sql/snowflakeHighlight.test.ts` 4, `src/features/database/sql/sql.test.ts` 43.
- Never in tests or the real window: TOOL (SQLite only) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 6107 lines, write about 2269; split into 2 by the native neighbours it touches.
- Depends on: none.

### 48. `F-DB-TREE-SOURCES-A` Database tree and sources: introspection, objects, the tree, table editor and table dialogs

- Webview sources (lines): `src/features/database/DatabaseView.tsx` (664), `src/features/database/introspection.ts` (217), `src/features/database/objects.ts` (378); spec files 1482 lines.
- Native neighbours: `app-gpui/src/database/tree_view.rs`, `app-gpui/src/database/source_dialog.rs`, `app-gpui/src/database/export.rs`, `app-gpui/src/database/objects.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:db.ddlToClipboard`, `act:db.ddlToConsole`, `act:db.editData`, `act:db.expandAll`, `act:db.introspect`, `act:db.modifyTable`, `act:db.newColumn`, `act:db.newTable`, `act:db.selectOpened`, `kind:db-table`, `ipc:db_database_catalog`, `ipc:db_databases`, `ipc:db_retain_catalog`, `ipc:db_saved_catalog`, `ipc:db_schema_objects`, `unit:src/features/database/database.test.ts`, `unit:src/features/database/DatabaseView.test.tsx`, `unit:src/features/database/introspection.test.ts`, `unit:src/features/database/objects.test.ts`, `unit:src/features/database/table-editor.test.tsx`, `e2e:e2e/databaseDialogLayout.spec.ts`.
- Spec files and cases (total cases at HEAD; 29 still to prove): `e2e/databaseDialogLayout.spec.ts` 1, `src/features/database/DatabaseView.test.tsx` 7, `src/features/database/database.test.ts` 35, `src/features/database/introspection.test.ts` 9, `src/features/database/objects.test.ts` 20, `src/features/database/table-editor.test.tsx` 6.
- Never in tests or the real window: TOOL (SQLite), NET (no server or SSH tunnel; tunnel test against a refused loopback port), SECRET (passwords and SSH secrets) (NET: no network; TOOL: no machine toolchain or real external program; SECRET: secret values never in Debug, logs or failure text).
- Size (Hypothesis): read about 2741 lines, write about 1922; split into 2: introspection + objects, then export + compare + sources.
- Depends on: none.

### 49. `F-WB-SHELL-B` Workbench shell: the workbench's extension points and remaining modules (each closes in a few lines once its user exists)

- Webview sources (lines): `src/workbench/activate.ts` (108), `src/workbench/browserGuards.ts` (65), `src/workbench/contributions.ts` (2086), `src/workbench/devSamples.tsx` (16), `src/workbench/EditorArea.tsx` (70), `src/workbench/ErrorBoundary.tsx` (31), `src/workbench/fileColors.ts` (31), `src/workbench/fileDrop.ts` (87), `src/workbench/OverlayHost.tsx` (25), `src/workbench/perfProbe.ts` (254), `src/workbench/projectColor.ts` (65), `src/workbench/projectWindow.ts` (77), `src/workbench/providerAcquisition.ts` (63), `src/workbench/quickSwitch.ts` (152), `src/workbench/shellActions.ts` (150), `src/workbench/theme.ts` (299), `src/workbench/Workbench.tsx` (123); spec files 0 lines.
- Native neighbours: `app-gpui/src/workbench/mod.rs`, `app-gpui/src/workbench/quick_switch.rs`, `app-gpui/src/platform_more/file_drop.rs`, `app-gpui/src/platform_more/file_editors.rs`, `app-gpui/src/platform_more/provider_acquisition.rs`, `app-gpui/src/platform_more/occurrences.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:(at) src/workbench/contributions.ts:1628`, `pt:(registered at run time only)`, `pt:(registered at run time only)`, `mod:src/workbench/activate.ts`, `mod:src/workbench/browserGuards.ts`, `mod:src/workbench/contributions.ts`, `mod:src/workbench/devSamples.tsx`, `mod:src/workbench/EditorArea.tsx`, `mod:src/workbench/ErrorBoundary.tsx`, `mod:src/workbench/fileColors.ts`, `mod:src/workbench/fileDrop.ts`, `mod:src/workbench/OverlayHost.tsx`, `mod:src/workbench/perfProbe.ts`, `mod:src/workbench/projectColor.ts`, `mod:src/workbench/projectWindow.ts`, `mod:src/workbench/providerAcquisition.ts`, `mod:src/workbench/quickSwitch.ts`, `mod:src/workbench/shellActions.ts`, `mod:src/workbench/theme.ts`, `mod:src/workbench/Workbench.tsx`, `ipc:window_open_remote`.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 3702 lines, write about 2315; split into 2: Workbench + project window + quick switch, then the extension points (each a few lines once the feature using it exists).
- Depends on: none.

### 50. `F-BT-SBT-ANALYZER-B` sbt tool window, Maven/Gradle Dependency Analyzer, Add/Unlink Maven projects, generate sources, skip tests, auto-reload, first import, profile reload: sbt tool window, auto-reload, first import, profile reload, generate sources, skip tests

- Webview sources (lines): `src/features/build-tools/model.ts` (665), `src/features/build-tools/sbt.ts` (117); spec files 1154 lines.
- Native neighbours: `app-gpui/src/build_tools`, `app-gpui/src/workbench/build_tools.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:gradle.toggleSkipTests`, `act:maven.generateSources`, `act:sbt.collapseAll`, `act:sbt.expandAll`, `act:sbt.reload`, `ctx:build.sbt`, `ctx:GRADLE_WHEN`, `ctx:MAVEN_WHEN`, `tw:sbt`, `ipc:build_sync`, `unit:src/features/build-tools/actions.test.tsx`, `unit:src/features/build-tools/autoReload.test.ts`, `unit:src/features/build-tools/firstImport.test.ts`, `unit:src/features/build-tools/model.test.ts`, `unit:src/features/build-tools/profileReload.test.ts`, `unit:src/features/build-tools/sbt.test.ts`, `e2e:e2e/linkedGradle.spec.ts`.
- Spec files and cases (total cases at HEAD; 13 still to prove): `e2e/linkedGradle.spec.ts` 1, `src/features/build-tools/actions.test.tsx` 5, `src/features/build-tools/autoReload.test.ts` 3, `src/features/build-tools/firstImport.test.ts` 2, `src/features/build-tools/model.test.ts` 18, `src/features/build-tools/profileReload.test.ts` 3, `src/features/build-tools/sbt.test.ts` 2.
- Never in tests or the real window: TOOL (no Maven, Gradle or sbt runs: the backend's switches stay off and answers are recorded), HOME (no ~/.m2, ~/.gradle, mise config) (TOOL: no machine toolchain or real external program; HOME: nothing of the user's home).
- Size (Hypothesis): read about 1936 lines, write about 1437; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 82 %; Hypothesis).
- Depends on: none.

### 51. `F-VCS-REPO-DIALOGS-B` Git dialogs pick the repository (merge, pull, rebase, tag, push), operation repository, multi-root: push: changes, single push, targets, target branch input, rejected push, remote results, update project

- Webview sources (lines): `src/features/vcs/pushChanges.tsx` (160), `src/features/vcs/targetBranchInput.tsx` (171); spec files 1079 lines.
- Native neighbours: `app-gpui/src/vcs/git_forms.rs`, `app-gpui/src/vcs_ops/branch_ops.rs`, `app-gpui/src/vcs_more/push_target.rs`, `app-gpui/src/vcs_more/multi_root.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vcs.manageRemotes`, `act:vcs.push`, `unit:src/features/vcs/__tests__/multiPushTarget.test.tsx`, `unit:src/features/vcs/__tests__/pushChanges.test.tsx`, `unit:src/features/vcs/__tests__/pushRejected.test.ts`, `unit:src/features/vcs/__tests__/remoteResults.test.ts`, `unit:src/features/vcs/__tests__/singlePush.test.tsx`, `unit:src/features/vcs/__tests__/targetBranchInput.test.tsx`, `e2e:e2e/pushTargetBranch.spec.ts`, `e2e:e2e/updateProject.spec.ts`.
- Spec files and cases (total cases at HEAD; 39 still to prove): `e2e/pushTargetBranch.spec.ts` 6, `e2e/updateProject.spec.ts` 1, `src/features/vcs/__tests__/multiPushTarget.test.tsx` 7, `src/features/vcs/__tests__/pushChanges.test.tsx` 4, `src/features/vcs/__tests__/pushRejected.test.ts` 6, `src/features/vcs/__tests__/remoteResults.test.ts` 14, `src/features/vcs/__tests__/singlePush.test.tsx` 2, `src/features/vcs/__tests__/targetBranchInput.test.tsx` 10.
- Never in tests or the real window: GIT (a bare repository in the temp folder stands for the remote) (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 1410 lines, write about 877; one writer.
- Depends on: Q-FU4.

### 52. `F-PLAT-MODELS-C-A` Platform models III: i18n, text catalog, date formats, release notes, splash, boot

- Webview sources (lines): `src/platform/dateFormat.ts` (101), `src/platform/releaseNotes.ts` (78), `src/platform/splash.ts` (68), `src/platform/i18n.ts` (489); spec files 1564 lines.
- Native neighbours: `app-gpui/src/platform/i18n.rs`, `app-gpui/src/platform/date_format.rs`, `app-gpui/src/platform_more/boot.rs`, `app-gpui/src/platform/error_log_sink.rs`, `app-gpui/src/platform_more/proxy_settings.rs`, `app-gpui/src/platform_more/workflow_settings.rs`, `app-gpui/src/platform_more/client_settings.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `mod:src/platform/dateFormat.ts`, `mod:src/platform/releaseNotes.ts`, `mod:src/platform/splash.ts`, `unit:src/platform/boot.test.ts`, `unit:src/platform/dateFormat.test.ts`, `unit:src/platform/i18n.test.ts`, `unit:src/platform/locales/textCatalog.test.ts`, `unit:src/platform/releaseNotes.test.ts`, `unit:src/platform/splash.test.ts`, `e2e:e2e/boot.spec.ts`.
- Spec files and cases (total cases at HEAD; 38 still to prove): `e2e/boot.spec.ts` 4, `src/platform/boot.test.ts` 21, `src/platform/dateFormat.test.ts` 3, `src/platform/i18n.test.ts` 32, `src/platform/locales/textCatalog.test.ts` 10, `src/platform/releaseNotes.test.ts` 8, `src/platform/splash.test.ts` 3.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 2300 lines, write about 1091; one writer.
- Depends on: Q-FU3 (date format item 9).

### 53. `F-PROJECT-VIEW` Project view: rename in place, Show Members, root paths, open in right split, Scratches and Unity panes, extra roots, Maven roots, module groups, clipboard of files, context menu

- Webview sources (lines): `src/features/project-tree/` (6867), `src/features/project-tree/ProjectView.tsx` (899), `src/features/project-tree/contextMenu.ts` (527), `src/features/project-tree/fileRefactor.ts` (334), `src/features/project-tree/fs.ts` (322), `src/features/project-tree/mavenRoots.ts` (61), `src/features/project-tree/model.ts` (653), `src/features/project-tree/moduleGroups.ts` (100), `src/features/project-tree/operations.ts` (923), `src/features/project-tree/projectTypes.ts` (78), `src/features/project-tree/store.ts` (789); spec files 3710 lines.
- Native neighbours: `app-gpui/src/project_view`, `app-gpui/src/project_tree.rs`, `app-gpui/src/workbench/project_view_actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:navigate.selectIn`, `act:navigate.showFilePath`, `act:project.copy`, `act:project.cut`, `act:project.openInRightSplit`, `act:project.option.showMembers`, `act:project.option.showRootPaths`, `act:project.paste`, `act:project.rename`, `act:project.renameInline`, `act:project.revealInFinder`, `act:project.viewMode.scratches`, `act:project.viewMode.unity`, `pt:(at) src/features/project-tree/index.ts:140`, `pt:(at) src/features/project-tree/index.ts:151`, `pt:(at) src/features/project-tree/index.ts:162`, `tw:project`, `pt:(at) src/features/project-tree/index.ts:242`, `feat:project-tree`, `ipc:clipboard_has_files`, `ipc:clipboard_has_image`, `ipc:clipboard_set_file_list`, `ipc:fs_rename`, `ipc:remote_import_local`, `unit:src/features/project-tree/contextMenu.test.ts`, `unit:src/features/project-tree/extraRoots.test.ts`, `unit:src/features/project-tree/fileRefactor.test.tsx`, `unit:src/features/project-tree/fs.test.ts`, `unit:src/features/project-tree/mavenRootIcon.test.tsx`, `unit:src/features/project-tree/mavenRoots.test.ts`, `unit:src/features/project-tree/model.test.ts`, `unit:src/features/project-tree/moduleGroups.test.ts`, `unit:src/features/project-tree/operations.test.ts`, `unit:src/features/project-tree/projectType.test.ts`, `unit:src/features/project-tree/projectTypes.test.ts`, `unit:src/features/project-tree/ProjectView.test.tsx`, `unit:src/features/project-tree/selectIn.test.ts`, `unit:src/features/project-tree/store.test.ts`, `unit:src/features/project-tree/unityRefresh.test.ts`, `e2e:e2e/projectTree.spec.ts`.
- Spec files and cases (total cases at HEAD; 88 still to prove): `e2e/projectTree.spec.ts` 6, `src/features/project-tree/ProjectView.test.tsx` 34, `src/features/project-tree/contextMenu.test.ts` 19, `src/features/project-tree/extraRoots.test.ts` 8, `src/features/project-tree/fileRefactor.test.tsx` 7, `src/features/project-tree/fs.test.ts` 4, `src/features/project-tree/mavenRootIcon.test.tsx` 1, `src/features/project-tree/mavenRoots.test.ts` 2, `src/features/project-tree/model.test.ts` 20, `src/features/project-tree/moduleGroups.test.ts` 6, `src/features/project-tree/operations.test.ts` 25, `src/features/project-tree/projectType.test.ts` 8, `src/features/project-tree/projectTypes.test.ts` 7, `src/features/project-tree/selectIn.test.ts` 7, `src/features/project-tree/store.test.ts` 21, `src/features/project-tree/unityRefresh.test.ts` 2.
- Never in tests or the real window: none (temp project).
- Size (Hypothesis): read about 15263 lines, write about 5007; split into 4: ProjectView spec (34 cases) + rename in place, then panes + extra roots, then operations + clipboard.
- Depends on: none (slices: I-ACTIONS-ENABLED).

### 54. `F-AG-SESSION-FEATURES` Agent session: selection toolbar (Quote, side conversation), find in conversation, focus input, retry, permission forms and command titles, queued messages, background stop

- Webview sources (lines): `src/features/agents/SessionEditor.tsx` (3560), `src/features/agents/SelectionToolbar.tsx` (155), `src/features/agents/FormCard.tsx` (183), `src/features/agents/permission.ts` (65), `src/features/agents/Markdown.tsx` (439), `src/features/agents/conversation.ts` (195), `src/features/agents/replyActions.ts` (170), `src/features/agents/reveal.ts` (87), `src/features/agents/symbolLinks.ts` (73), `src/features/agents/timeline.ts` (137); spec files 6850 lines.
- Native neighbours: `app-gpui/src/agents/session_view.rs`, `app-gpui/src/agents/cards.rs`, `app-gpui/src/agents/conversation.rs`, `app-gpui/src/agents/markdown_view.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:agents.findInConversation`, `act:agents.focusInput`, `act:agents.quoteReply`, `act:agents.retryConversation`, `act:agents.startSideConversation`, `ctx:agentInputAvailable`, `ipc:agent_create_ephemeral`, `ipc:agent_entries`, `ipc:agent_respond`, `ipc:agent_send_queued_now`, `ipc:agent_stop_background`, `ipc:agent_unqueue`, `unit:src/features/agents/conversation.test.ts`, `unit:src/features/agents/Markdown.test.tsx`, `unit:src/features/agents/mermaidFollow.test.tsx`, `unit:src/features/agents/messageEditing.test.tsx`, `unit:src/features/agents/permission.test.ts`, `unit:src/features/agents/replyActions.test.ts`, `unit:src/features/agents/reveal.test.tsx`, `unit:src/features/agents/SelectionToolbar.test.tsx`, `unit:src/features/agents/sessionState.test.tsx`, `unit:src/features/agents/sideConversation.test.ts`, `unit:src/features/agents/symbolLinks.test.ts`, `unit:src/features/agents/timeline.test.ts`, `e2e:e2e/agentApproval.spec.ts`, `e2e:e2e/agentFocusInput.spec.ts`, `e2e:e2e/agentMarkdown.spec.ts`, `e2e:e2e/agentQuestions.spec.ts`, `e2e:e2e/agentScroll.spec.ts`.
- Spec files and cases (total cases at HEAD; 94 still to prove): `e2e/agentApproval.spec.ts` 2, `e2e/agentFocusInput.spec.ts` 11, `e2e/agentMarkdown.spec.ts` 22, `e2e/agentQuestions.spec.ts` 1, `e2e/agentScroll.spec.ts` 3, `src/features/agents/Markdown.test.tsx` 28, `src/features/agents/SelectionToolbar.test.tsx` 9, `src/features/agents/conversation.test.ts` 13, `src/features/agents/mermaidFollow.test.tsx` 2, `src/features/agents/messageEditing.test.tsx` 18, `src/features/agents/permission.test.ts` 8, `src/features/agents/replyActions.test.ts` 12, `src/features/agents/reveal.test.tsx` 7, `src/features/agents/sessionState.test.tsx` 108, `src/features/agents/sideConversation.test.ts` 6, `src/features/agents/symbolLinks.test.ts` 4, `src/features/agents/timeline.test.ts` 3.
- Never in tests or the real window: AGENT (fake agent only) (AGENT: never start a coding agent).
- Size (Hypothesis): read about 11914 lines, write about 4334; split into 3: selection toolbar + side conversations, then permission forms + queue, then Markdown (28) + session state (108, partial).
- Depends on: none.

### 55. `F-DBG-ATTACH` Attach to Process, Unity attach, remote debug types (Go, Node, PHP, Python, JVM, Godot), adapter choice, missing adapter

- Webview sources (lines): `src/features/debugger/attach.ts` (104), `src/features/debugger/attachPopup.ts` (44), `src/features/debugger/remote.ts` (436), `src/features/debugger/missingAdapter.ts` (66); spec files 606 lines.
- Native neighbours: `app-gpui/src/debugger/attach.rs`, `app-gpui/src/debugger/remote.rs`, `app-gpui/src/debugger/missing_adapter.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:debug.attach`, `act:debug.attachToProcess`, `act:debug.attachUnity`, `pt:go-remote`, `pt:godot-attach`, `pt:node-remote`, `pt:php-remote`, `pt:python-remote`, `pt:remote-jvm`, `ipc:debug_attachable_processes`, `ipc:debug_unity_processes`, `unit:src/features/debugger/__tests__/adapterChoice.test.ts`, `unit:src/features/debugger/__tests__/attach.test.ts`, `unit:src/features/debugger/__tests__/missingAdapter.test.ts`, `unit:src/features/debugger/__tests__/remote.test.ts`, `unit:src/features/debugger/__tests__/unityAttach.test.tsx`.
- Spec files and cases (total cases at HEAD; 23 still to prove): `src/features/debugger/__tests__/adapterChoice.test.ts` 2, `src/features/debugger/__tests__/attach.test.ts` 5, `src/features/debugger/__tests__/missingAdapter.test.ts` 7, `src/features/debugger/__tests__/remote.test.ts` 18, `src/features/debugger/__tests__/unityAttach.test.tsx` 6.
- Never in tests or the real window: TOOL (the process list is a stand-in; nothing is attached to a real process), DEBUG (TOOL: no machine toolchain or real external program; DEBUG: stand-in debug adapter only).
- Size (Hypothesis): read about 1256 lines, write about 1481; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 81 %; Hypothesis).
- Depends on: Q-FU2 (item 8).

### 56. `F-LSP-NAV` Language navigation: chooser, shared navigation, implementations, imported declarations, pending navigation, mouse navigation, document links, file references

- Webview sources (lines): `src/editor/lsp/configurationNavigation.ts` (174), `src/editor/lsp/fileReferences.ts` (159), `src/editor/lsp/links.ts` (218), `src/editor/lsp/navigation.ts` (1412); spec files 1681 lines.
- Native neighbours: `app-gpui/src/editor_lsp/navigation.rs`, `app-gpui/src/editor_lsp/links.rs`, `app-gpui/src/editor_lsp/file_references.rs`, `app-gpui/src/editor_lsp/configuration_navigation.rs`, `app-gpui/src/editor_lsp/targets_view.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:navigate.declaration`, `act:navigate.implementation`, `mod:src/editor/lsp/configurationNavigation.ts`, `mod:src/editor/lsp/fileReferences.ts`, `mod:src/editor/lsp/links.ts`, `mod:src/editor/lsp/navigation.ts`, `ipc:fs_resolve_reference`, `ipc:lsp_document_links`, `ipc:lsp_goto_implementation`, `ipc:nav_index_definitions`, `unit:src/editor/lsp/configurationNavigation.test.ts`, `unit:src/editor/lsp/links.test.ts`, `unit:src/editor/lsp/navigationChooser.test.ts`, `unit:src/editor/lsp/navigationImplementations.test.ts`, `unit:src/editor/lsp/navigationImportedDeclaration.test.ts`, `unit:src/editor/lsp/navigationMouse.test.ts`, `unit:src/editor/lsp/navigationPending.test.ts`, `unit:src/editor/lsp/navigationShared.test.ts`, `e2e:e2e/configurationNavigation.spec.ts`.
- Spec files and cases (total cases at HEAD; 51 still to prove): `e2e/configurationNavigation.spec.ts` 3, `src/editor/lsp/configurationNavigation.test.ts` 18, `src/editor/lsp/links.test.ts` 10, `src/editor/lsp/navigationChooser.test.ts` 10, `src/editor/lsp/navigationImplementations.test.ts` 3, `src/editor/lsp/navigationImportedDeclaration.test.ts` 6, `src/editor/lsp/navigationMouse.test.ts` 14, `src/editor/lsp/navigationPending.test.ts` 10, `src/editor/lsp/navigationShared.test.ts` 16.
- Never in tests or the real window: TOOL (stand-in language server) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 3644 lines, write about 2145; split into 2 by the native neighbours it touches.
- Depends on: Q-FU3.

### 57. `F-SVC-DOCKER-B` Services: Compose (up, gutter, file override, auto-detect), dashboard, connection dialog, the rest

- Webview sources (lines): `src/features/services/compose.ts` (427), `src/features/services/dockerGutter.ts` (155); spec files 1332 lines.
- Native neighbours: `app-gpui/src/services`, `app-gpui/src/workbench/services_window.rs`, `app-gpui/src/workbench/services_actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:docker.composeUp`, `act:docker.setComposeFileOverride`, `act:docker.showDashboard`, `act:services.addService`, `pt:docker.composeAutoDetect`, `pt:services.composeEditor`, `pt:services.dockerGutter`, `ovl:services.dialogs`, `pt:docker-compose`, `pt:docker-dockerfile`, `pt:docker`, `tw:services`, `feat:services`, `unit:src/features/services/compose.test.ts`, `unit:src/features/services/connectionDialog.test.tsx`, `unit:src/features/services/dockerGutter.test.ts`, `unit:src/features/services/services.test.ts`, `unit:src/features/services/ServicesTree.debug.test.tsx`, `unit:src/features/services/ServicesTree.reorder.test.tsx`.
- Spec files and cases (total cases at HEAD; 26 still to prove): `src/features/services/ServicesTree.debug.test.tsx` 10, `src/features/services/ServicesTree.reorder.test.tsx` 5, `src/features/services/compose.test.ts` 6, `src/features/services/connectionDialog.test.tsx` 1, `src/features/services/dockerGutter.test.ts` 1, `src/features/services/services.test.ts` 26.
- Never in tests or the real window: DOCKER (never the machine's daemon: a stand-in Docker API on a temp socket; the daemon rows' parity rests on it), NET (no registry login or pull) (NET: no network; DOCKER: never the Docker daemon).
- Size (Hypothesis): read about 1914 lines, write about 1571; split into 2: containers + actions, then Compose + gutter, then dashboard.
- Depends on: none.
- Note: The docker_* commands are in C-TOOLCHAIN; this slice proves the UI against a stand-in.

### 58. `F-VCS-DIFF-FIND` Diff viewer: find bar, chains, providers, external diff tool, blank diff, save from the diff window

- Webview sources (lines): `src/features/vcs/diffInput.ts` (365), `src/features/vcs/diffViewerProviders.ts` (95), `src/features/vcs/diffWindowSave.ts` (56), `src/features/vcs/compareTool.tsx` (105); spec files 1736 lines.
- Native neighbours: `app-gpui/src/vcs_diff/dialog_view.rs`, `app-gpui/src/vcs_diff/tab_view.rs`, `app-gpui/src/workbench/diff_window.rs`, `app-gpui/src/workbench/diff_tools.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:diff.closeEditor`, `act:diff.compareWith`, `act:diff.openBlankDiff`, `act:diff.showInExternalTool`, `kind:diff`, `set:tools.externalDiff`, `ipc:diff_external_tool`, `ipc:doc_snapshot`, `ipc:window_open_diff`, `unit:src/features/vcs/__tests__/compareTool.test.tsx`, `unit:src/features/vcs/__tests__/diffChain.test.ts`, `unit:src/features/vcs/__tests__/diffFind.test.tsx`, `unit:src/features/vcs/__tests__/diffViewerProviders.test.tsx`, `unit:src/features/vcs/__tests__/modalDiffWindow.test.ts`, `unit:src/features/vcs/__tests__/show-diff-chain.test.ts`, `unit:src/features/vcs/diffWindowSave.test.ts`, `e2e:e2e/compareWith.spec.ts`, `e2e:e2e/diff.spec.ts`, `e2e:e2e/diffJumpToSource.spec.ts`.
- Spec files and cases (total cases at HEAD; 49 still to prove): `e2e/compareWith.spec.ts` 4, `e2e/diff.spec.ts` 11, `e2e/diffJumpToSource.spec.ts` 1, `src/features/vcs/__tests__/compareTool.test.tsx` 7, `src/features/vcs/__tests__/diffChain.test.ts` 7, `src/features/vcs/__tests__/diffFind.test.tsx` 16, `src/features/vcs/__tests__/diffViewerProviders.test.tsx` 5, `src/features/vcs/__tests__/modalDiffWindow.test.ts` 5, `src/features/vcs/__tests__/show-diff-chain.test.ts` 17, `src/features/vcs/diffWindowSave.test.ts` 3.
- Never in tests or the real window: TOOL (the external diff tool is a stand-in script in the temp folder, never the user's configured tool) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 2357 lines, write about 1819; split into 2 by the native neighbours it touches.
- Depends on: none (slices: I-ACTIONS-ENABLED).

### 59. `F-WB-TOOLWINDOWS` Tool window store and layouts: maximize, view modes, layouts (delete, rename, restore), focus, narrow window, Zen, distraction-free

- Webview sources (lines): `src/workbench/distractionFree.ts` (145), `src/workbench/focus.ts` (92), `src/workbench/narrowWindow.ts` (25), `src/workbench/toolwindows/actions.ts` (322), `src/workbench/toolwindows/store.ts` (819), `src/workbench/toolwindows/ToolWindows.tsx` (1376), `src/workbench/toolwindows/layouts.ts` (114); spec files 1620 lines.
- Native neighbours: `app-gpui/src/workbench/tool_windows.rs`, `app-gpui/src/platform_more/focus.rs`, `app-gpui/src/platform_more/distraction_free.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:editor.toggleZen`, `act:layout.delete`, `act:layout.rename`, `act:layout.restore`, `ctx:toolWindowLayoutActive`, `mod:src/workbench/distractionFree.ts`, `mod:src/workbench/focus.ts`, `mod:src/workbench/narrowWindow.ts`, `mod:src/workbench/toolwindows/actions.ts`, `mod:src/workbench/toolwindows/store.ts`, `mod:src/workbench/toolwindows/ToolWindows.tsx`, `unit:src/workbench/focus.test.ts`, `unit:src/workbench/narrowWindow.test.ts`, `unit:src/workbench/toolwindows/layouts.test.ts`, `unit:src/workbench/toolwindows/store.test.ts`, `e2e:e2e/stripeTooltip.spec.ts`, `e2e:e2e/toolwindows.spec.ts`, `e2e:e2e/treeWidth.spec.ts`, `e2e:e2e/workbench.spec.ts`.
- Spec files and cases (total cases at HEAD; 45 still to prove): `e2e/stripeTooltip.spec.ts` 2, `e2e/toolwindows.spec.ts` 11, `e2e/treeWidth.spec.ts` 6, `e2e/workbench.spec.ts` 3, `src/workbench/focus.test.ts` 7, `src/workbench/narrowWindow.test.ts` 5, `src/workbench/toolwindows/layouts.test.ts` 7, `src/workbench/toolwindows/store.test.ts` 44.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 4513 lines, write about 2242; split into 2: store (18 missing cases), then layouts and focus.
- Depends on: I-TW-CONTENT-TABS (slices: I-TW-CONTENT-TABS).

### 60. `F-NFT-REST` What New from Template (landed in D149, 11014c6eb) left partial: the remaining cases of its specs and rows

- Webview sources (lines): `src/features/templates/files` (1687), `src/features/templates/index.ts` (287), `src/platform/fileTemplates.ts` (37); spec files 649 lines.
- Native neighbours: `app-gpui/src/templates`, `app-gpui/src/workbench/template_files.rs`, `app-gpui/src/workbench/template_actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:(dynamic) src/features/templates/index.ts:81`, `act:templates.newFromTemplate`, `pt:templates.new`, `ovl:templates.chooser`, `ovl:templates.newFile`, `mod:src/platform/fileTemplates.ts`, `unit:src/features/templates/__tests__/files.test.ts`, `unit:src/features/templates/__tests__/pluginFiles.test.tsx`, `unit:src/features/templates/__tests__/templateCreate.test.ts`.
- Spec files and cases (total cases at HEAD; 21 still to prove): `src/features/templates/__tests__/files.test.ts` 32, `src/features/templates/__tests__/pluginFiles.test.tsx` 6, `src/features/templates/__tests__/templateCreate.test.ts` 6.
- Never in tests or the real window: HOME (files are created only in the temp project) (HOME: nothing of the user's home).
- Size (Hypothesis): read about 2660 lines, write about 785; one writer.
- Depends on: none.
- Note: Read `docs/gpui-port/notes/reports/int_templates_files.md` (the integrator's report) and each row's proof first: they name what is left.

### 61. `F-VCS-MERGE` Merge editor, conflict markers in the editor, Git merge diff

- Webview sources (lines): `src/features/vcs/MergeEditor.tsx` (639), `src/features/vcs/GitMergeDiff.tsx` (98), `src/features/vcs/conflictMarkers.ts` (70), `src/features/vcs/conflicts.ts` (161); spec files 579 lines.
- Native neighbours: `app-gpui/src/vcs/merge_view.rs`, `app-gpui/src/vcs_diff/conflict_markers.rs`, `app-gpui/src/vcs_more/merge_model.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vcs.resolveConflicts`, `pt:merge-conflict-markers`, `kind:git-merge-diff`, `kind:merge`, `unit:src/features/vcs/__tests__/conflictMarkerEdits.test.ts`, `unit:src/features/vcs/__tests__/conflictMarkers.test.ts`, `unit:src/features/vcs/__tests__/GitMergeDiff.test.tsx`, `e2e:e2e/conflictMarkers.spec.ts`, `e2e:e2e/mergeEditor.spec.ts`.
- Spec files and cases (total cases at HEAD; 13 still to prove): `e2e/conflictMarkers.spec.ts` 1, `e2e/mergeEditor.spec.ts` 19, `src/features/vcs/__tests__/GitMergeDiff.test.tsx` 1, `src/features/vcs/__tests__/conflictMarkerEdits.test.ts` 1, `src/features/vcs/__tests__/conflictMarkers.test.ts` 5.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 1547 lines, write about 902; one writer.
- Depends on: none.

### 62. `F-LSP-POPUPS-B` Completion, hover, docs, signature, intentions, quick fixes, type info, hint, Popups, auto import and imports on paste, in-place rename, outside-project files: hover, docs, signature, Popups, Markdown, hint, type info

- Webview sources (lines): `src/editor/lsp/docs.ts` (166), `src/editor/lsp/hover.ts` (396), `src/editor/lsp/markdown.ts` (281), `src/editor/lsp/markdownParser.ts` (19), `src/editor/lsp/Popups.tsx` (779), `src/editor/lsp/popupStore.ts` (193), `src/editor/lsp/signature.ts` (296), `src/editor/lsp/typeInfo.ts` (82), `src/editor/lsp/hint.ts` (122); spec files 489 lines.
- Native neighbours: `app-gpui/src/editor_lsp/completion.rs`, `app-gpui/src/editor_lsp/hover.rs`, `app-gpui/src/editor_lsp/signature.rs`, `app-gpui/src/editor_lsp/intentions.rs`, `app-gpui/src/editor_lsp/inplace_rename.rs`, `app-gpui/src/editor_lsp/outside_project.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:code.expressionTypeInfo`, `act:code.parameterInfo`, `act:code.quickDocumentation`, `act:code.quickTypeDefinition`, `ovl:editor-lsp.popups`, `tw:documentation`, `mod:src/editor/lsp/docs.ts`, `mod:src/editor/lsp/hover.ts`, `mod:src/editor/lsp/markdown.ts`, `mod:src/editor/lsp/markdownParser.ts`, `mod:src/editor/lsp/Popups.tsx`, `mod:src/editor/lsp/popupStore.ts`, `mod:src/editor/lsp/signature.ts`, `mod:src/editor/lsp/typeInfo.ts`, `unit:src/editor/lsp/hint.test.ts`, `unit:src/editor/lsp/markdown.test.ts`, `unit:src/editor/lsp/Popups.test.tsx`, `e2e:e2e/hover-clamp.spec.ts`.
- Spec files and cases (total cases at HEAD; 14 still to prove): `e2e/hover-clamp.spec.ts` 1, `src/editor/lsp/Popups.test.tsx` 14, `src/editor/lsp/hint.test.ts` 7, `src/editor/lsp/markdown.test.ts` 6.
- Never in tests or the real window: TOOL (stand-in language server) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 2823 lines, write about 1788; split into 2: completion (55 cases) alone; then hover/docs/signature; then intentions/rename/imports.
- Depends on: Q-FU3, Q-FU4.

### 63. `F-WB-STATUS-TASKS` Status bar: unavailable widgets, info item, tasks, background processes (stop), running processes provider

- Webview sources (lines): `src/workbench/confirmTerminate.ts` (26), `src/workbench/StatusBar.tsx` (182), `src/workbench/statusBarWidgets.ts` (96), `src/workbench/tasks.ts` (219); spec files 426 lines.
- Native neighbours: `app-gpui/src/workbench/status_bar.rs`, `app-gpui/src/workbench/status_bar_widgets.rs`, `app-gpui/src/workbench/tasks.rs`, `app-gpui/src/platform_more/running_processes.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:workbench.stopBackgroundProcesses`, `stat:workbench.info`, `mod:src/workbench/confirmTerminate.ts`, `mod:src/workbench/StatusBar.tsx`, `mod:src/workbench/statusBarWidgets.ts`, `unit:src/workbench/runningProcesses.test.ts`, `unit:src/workbench/statusBarWidgets.test.tsx`, `unit:src/workbench/stopBackground.test.ts`, `unit:src/workbench/tasks.test.tsx`.
- Spec files and cases (total cases at HEAD; 6 still to prove): `src/workbench/runningProcesses.test.ts` 3, `src/workbench/statusBarWidgets.test.tsx` 5, `src/workbench/stopBackground.test.ts` 1, `src/workbench/tasks.test.tsx` 10.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 949 lines, write about 561; one writer.
- Depends on: Q-FU3.

### 64. `F-APP-WORKSPACE` Workspaces: create, manage, open, attach/detach projects, discover projects, workspace dialog, project types

- Webview sources (lines): `src/features/app/WorkspaceDialog.tsx` (322), `src/features/app/projectActions.ts` (277), `src/ui/projectType.ts` (58); spec files 594 lines.
- Native neighbours: `app-gpui/src/app_models/workspace_dialog.rs`, `app-gpui/src/app_models/project_actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:project.attach`, `act:project.detach`, `act:workspace.create`, `act:workspace.manage`, `act:workspace.open`, `act:workspace.removeProject`, `ovl:app.workspace`, `mod:src/ui/projectType.ts`, `ipc:workspace_attach`, `ipc:workspace_create`, `ipc:workspace_discover_projects`, `ipc:workspace_update`, `unit:src/features/app/__tests__/WorkspaceDialog.test.tsx`, `e2e:e2e/attachProject.spec.ts`, `e2e:e2e/workspaceProjectTypes.spec.ts`.
- Spec files and cases (total cases at HEAD; 6 still to prove): `e2e/attachProject.spec.ts` 3, `e2e/workspaceProjectTypes.spec.ts` 2, `src/features/app/__tests__/WorkspaceDialog.test.tsx` 9.
- Never in tests or the real window: HOME (temp workspace folder) (HOME: nothing of the user's home).
- Size (Hypothesis): read about 1251 lines, write about 1477; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 93 %; Hypothesis).
- Depends on: none.

### 65. `F-WB-MENUS-TITLE` Main menu (interaction, toolbar style row), title bar (New Project…, Create Repository fallback), native title bar, window controls and drag, input dialogs, main toolbar dialog

- Webview sources (lines): `src/workbench/ContributedToolbar.tsx` (26), `src/workbench/inputDialogs.tsx` (337), `src/workbench/MainMenu.tsx` (305), `src/workbench/MainToolbarDialog.tsx` (228), `src/workbench/nativeTitlebar.ts` (47), `src/workbench/TitleBar.tsx` (385), `src/workbench/useFullscreen.ts` (28), `src/workbench/window-drag.ts` (86), `src/workbench/WindowControls.tsx` (82), `src/workbench/WindowResizeHandles.tsx` (77); spec files 1369 lines.
- Native neighbours: `app-gpui/src/workbench/title_bar.rs`, `app-gpui/src/workbench/menu.rs`, `app-gpui/src/workbench/menu_spec.rs`, `app-gpui/src/workbench/main_toolbar.rs`, `app-gpui/src/workbench/chrome.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:find.field.newLine`, `act:find.field.recentSearches`, `ctx:searchFieldHistory`, `ctx:searchFieldNewLine`, `mod:src/workbench/ContributedToolbar.tsx`, `mod:src/workbench/inputDialogs.tsx`, `mod:src/workbench/MainMenu.tsx`, `mod:src/workbench/MainToolbarDialog.tsx`, `mod:src/workbench/nativeTitlebar.ts`, `mod:src/workbench/TitleBar.tsx`, `mod:src/workbench/useFullscreen.ts`, `mod:src/workbench/window-drag.ts`, `mod:src/workbench/WindowControls.tsx`, `mod:src/workbench/WindowResizeHandles.tsx`, `shell:src-tauri/src/menu.rs`, `shell:src-tauri/src/titlebar.rs`, `ipc:window_is_fullscreen`, `ipc:window_start_drag`, `evt:WindowFullscreenChanged`, `unit:src/workbench/inputDialogs.test.tsx`, `unit:src/workbench/MainMenu.interaction.test.tsx`, `unit:src/workbench/MainMenu.test.ts`, `unit:src/workbench/nativeTitlebar.test.tsx`, `unit:src/workbench/TitleBar.test.tsx`, `unit:src/workbench/window-drag.test.ts`, `e2e:e2e/dialogFocus.spec.ts`, `e2e:e2e/dialogTitleBar.spec.ts`, `e2e:e2e/editorGitMenu.spec.ts`, `e2e:e2e/projectNewMenu.spec.ts`, `e2e:e2e/toolbarHover.spec.ts`.
- Spec files and cases (total cases at HEAD; 47 still to prove): `e2e/dialogFocus.spec.ts` 1, `e2e/dialogTitleBar.spec.ts` 2, `e2e/editorGitMenu.spec.ts` 2, `e2e/projectNewMenu.spec.ts` 2, `e2e/toolbarHover.spec.ts` 1, `src/workbench/MainMenu.interaction.test.tsx` 12, `src/workbench/MainMenu.test.ts` 3, `src/workbench/TitleBar.test.tsx` 9, `src/workbench/inputDialogs.test.tsx` 9, `src/workbench/nativeTitlebar.test.tsx` 7, `src/workbench/window-drag.test.ts` 5.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 2970 lines, write about 2546; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 83 %; Hypothesis).
- Depends on: I-ACTIONS-ENABLED (slices: I-ACTIONS-ENABLED).

### 66. `F-PERF-MCP-SSH` Perf (memory indicator, overlay, clear caches, end process for own processes only), MCP requests, SSH sessions and askpass

- Webview sources (lines): `src/features/perf` (774), `src/features/mcp` (213), `src/features/ssh` (310), `src/features/mcp/requests.ts` (67); spec files 493 lines.
- Native neighbours: `app-gpui/src/features_b/perf.rs`, `app-gpui/src/features_b/mcp.rs`, `app-gpui/src/features_b/ssh.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:perf.clearCaches`, `act:perf.showMemory`, `act:perf.toggleMemoryIndicator`, `act:perf.toggleOverlay`, `act:ssh.configurations`, `act:ssh.startSession`, `ovl:mcp.confirm`, `ovl:perf.overlay`, `ovl:ssh.askpass`, `stat:(at) src/features/perf/index.ts:12`, `feat:mcp`, `feat:perf`, `feat:ssh`, `ipc:app_metrics`, `ipc:clear_caches`, `ipc:end_process`, `ipc:mcp_respond`, `ipc:mcp_set_focused_editor`, `ipc:mcp_ui_ready`, `ipc:ssh_askpass_answer`, `ipc:ssh_open_terminal`, `evt:McpRequest`, `evt:McpRequestClosed`, `evt:Askpass`, `evt:AskpassDone`, `unit:src/features/mcp/reporters.test.ts`, `unit:src/features/mcp/requests.test.tsx`, `unit:src/features/perf/__tests__/views.test.tsx`, `unit:src/features/ssh/ssh.test.ts`.
- Spec files and cases (total cases at HEAD; 14 still to prove): `src/features/mcp/reporters.test.ts` 5, `src/features/mcp/requests.test.tsx` 5, `src/features/perf/__tests__/views.test.tsx` 4, `src/features/ssh/ssh.test.ts` 10.
- Never in tests or the real window: NET (no SSH connection), SECRET (askpass answers), TOOL (End Process never offered for a process the app did not start) (NET: no network; TOOL: no machine toolchain or real external program; SECRET: secret values never in Debug, logs or failure text).
- Size (Hypothesis): read about 1857 lines, write about 2260; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 93 %; Hypothesis).
- Depends on: Q-FU2 (memory popup).

### 67. `F-VCS-COMMIT-DIALOG` Modal Commit dialog, commit from context selection, commit checks, plugin commit checks

- Webview sources (lines): `src/features/vcs/CommitDialog.tsx` (100), `src/features/vcs/commitUi.ts` (89), `src/features/vcs/commitChecks.tsx` (522), `src/features/vcs/commitCheckRegistry.ts` (81); spec files 995 lines.
- Native neighbours: `app-gpui/src/workbench/commit.rs`, `app-gpui/src/vcs_ops/commit_checks.rs`, `app-gpui/src/vcs/commit_box.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vcs.analysis`, `act:vcs.commit`, `act:vcs.commitAndPush`, `act:vcs.crlf`, `act:vcs.detachedHead`, `act:vcs.fileNames`, `act:vcs.largeFiles`, `act:vcs.todo`, `pt:(at) src/features/plugins/uiPoints.tsx:215`, `ipc:plugin_answer_commit`, `ipc:plugin_commit_checks`, `unit:src/features/vcs/__tests__/commitChecks.test.ts`, `unit:src/features/vcs/__tests__/commitMode.test.ts`, `unit:src/features/vcs/__tests__/pluginCommit.test.ts`, `unit:src/features/vcs/__tests__/workflow.test.ts`, `e2e:e2e/commit.spec.ts`, `e2e:e2e/gitLogCommitChecks.spec.ts`.
- Spec files and cases (total cases at HEAD; 23 still to prove): `e2e/commit.spec.ts` 2, `e2e/gitLogCommitChecks.spec.ts` 1, `src/features/vcs/__tests__/commitChecks.test.ts` 12, `src/features/vcs/__tests__/commitMode.test.ts` 15, `src/features/vcs/__tests__/pluginCommit.test.ts` 4, `src/features/vcs/__tests__/workflow.test.ts` 15.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 1787 lines, write about 1651; split into 2 by the native neighbours it touches.
- Depends on: none.

### 68. `F-LSP-INLINE-A` Inline completion, inlay hints and requests, code lens (server and author), interactive commands, format errors, spelling: inline completion and inlay hints

- Webview sources (lines): `src/editor/lsp/inlay.ts` (250), `src/editor/lsp/inlineCompletion.ts` (327); spec files 602 lines.
- Native neighbours: `app-gpui/src/editor_lsp/inline_completion.rs`, `app-gpui/src/editor_lsp/inlay.rs`, `app-gpui/src/editor_lsp/code_lens.rs`, `app-gpui/src/editor_lsp/spelling.rs`, `app-gpui/src/editor_lsp/format.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:editor.inlineCompletion.accept`, `act:editor.inlineCompletion.acceptWord`, `act:editor.inlineCompletion.dismiss`, `act:editor.inlineCompletion.next`, `act:editor.inlineCompletion.previous`, `act:editor.inlineCompletion.trigger`, `mod:src/editor/lsp/inlay.ts`, `mod:src/editor/lsp/inlineCompletion.ts`, `ipc:lsp_inlay_hints`, `ipc:lsp_inline_completion`, `unit:src/editor/lsp/inlay.test.ts`, `unit:src/editor/lsp/inlayRequests.test.ts`, `unit:src/editor/lsp/inlineCompletion.test.ts`, `e2e:e2e/inlayRequests.spec.ts`.
- Spec files and cases (total cases at HEAD; 15 still to prove): `e2e/inlayRequests.spec.ts` 1, `src/editor/lsp/inlay.test.ts` 9, `src/editor/lsp/inlayRequests.test.ts` 8, `src/editor/lsp/inlineCompletion.test.ts` 8.
- Never in tests or the real window: TOOL (stand-in language server; no real spell checker dictionary download) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 1179 lines, write about 1431; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 86 %; Hypothesis).
- Depends on: none.

### 69. `F-GH-REVIEW-A` GitHub pull requests: review mode and review diff comments

- Webview sources (lines): `src/features/github/diff/reviewDiff.tsx` (486); spec files 987 lines.
- Native neighbours: `app-gpui/src/github` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:github.pullRequest.diff.addComment`, `act:github.pullRequest.diff.addMultilineComment`, `act:github.pullRequest.diff.nextComment`, `act:github.pullRequest.diff.previousComment`, `act:github.pullRequest.diff.refreshReview`, `act:github.pullRequest.reviewMode`, `act:github.pullRequest.reviewMode.showDiff`, `act:github.pullRequest.submitReview`, `ctx:githubReviewDiff`, `ctx:githubReviewEditor`, `pt:github.reviewMode`, `pt:github.reviewMode`, `unit:src/features/github/diff/reviewDiff.test.tsx`, `unit:src/features/github/reviewMode/reviewMode.test.tsx`.
- Spec files and cases (total cases at HEAD; 2 still to prove): `src/features/github/diff/reviewDiff.test.tsx` 11, `src/features/github/reviewMode/reviewMode.test.tsx` 20.
- Never in tests or the real window: NET (no GitHub: the `gh` seam answers from fixtures; never a signed-in gh), SECRET (tokens) (NET: no network; SECRET: secret values never in Debug, logs or failure text).
- Size (Hypothesis): read about 1473 lines, write about 1406; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 86 %; Hypothesis).
- Depends on: none.
- Note: Hypothesis: the five gh_pr_* commands have no webview caller (callers.py); settle with `git grep -n -E 'ghPr(Comment|List|Review|ReviewComments|View)' -- src` before planning them, they may belong to `[–]`.

### 70. `F-VCS-BRANCHES` Branches popup and widget (expansion, resize, host), branch widget extensions

- Webview sources (lines): `src/features/vcs/BranchWidget.tsx` (987), `src/features/vcs/branches.ts` (467), `src/features/vcs/branchMenu.ts` (205), `src/features/vcs/branchWidgetExtensions.ts` (60); spec files 522 lines.
- Native neighbours: `app-gpui/src/vcs/branches_popup.rs`, `app-gpui/src/vcs/branch_widget_view.rs`, `app-gpui/src/vcs_ops/branch_widget_extensions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vcs.branches`, `pt:(at) src/features/vcs/BranchWidget.tsx:368`, `ovl:vcs.branches`, `title:vcs.branch`, `unit:src/features/vcs/__tests__/branches.test.ts`, `unit:src/features/vcs/__tests__/branchesPopupHost.test.tsx`, `e2e:e2e/branchesPopupExpansion.spec.ts`, `e2e:e2e/branchesPopupResize.spec.ts`.
- Spec files and cases (total cases at HEAD; 5 still to prove): `e2e/branchesPopupExpansion.spec.ts` 1, `e2e/branchesPopupResize.spec.ts` 1, `src/features/vcs/__tests__/branches.test.ts` 14, `src/features/vcs/__tests__/branchesPopupHost.test.tsx` 3.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 2241 lines, write about 841; one writer.
- Depends on: none.

### 71. `F-EDITOR-CORE-EDITING` Editor editing: smart keys, editing, clipboard, string clipboard, doc comments, indent guides, lines, IDEA parity, editing extras, paste history

- Webview sources (lines): `src/editor/core/actions.ts` (1290), `src/editor/core/cm` (14414), `src/editor/core/extras.ts` (114), `src/editor/core/cm/clipboard.ts` (105), `src/editor/core/cm/docComment.ts` (146), `src/editor/core/cm/docCommentTags.ts` (227), `src/editor/core/cm/editing.ts` (849), `src/editor/core/cm/indentGuides.ts` (241), `src/editor/core/cm/lines.ts` (85), `src/editor/core/cm/smartKeys.ts` (389), `src/editor/core/cm/stringClipboard.ts` (160), `src/editor/core/view/PasteHistoryDialog.tsx` (243); spec files 3585 lines.
- Native neighbours: `app-gpui/src/editor_commands`, `app-gpui/src/editor/view`, `app-gpui/src/editor/paste_history_view`, `app-gpui/src/workbench/editor_core.rs`, `app-gpui/src/workbench/editor_extras.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:code.completeStatement`, `act:code.moveStatementDown`, `act:code.moveStatementUp`, `act:code.pasteFromHistory`, `ctx:pasteHistoryListFocus`, `ctx:TEXT_WHEN`, `ovl:editor.pasteHistory`, `mod:src/editor/core/actions.ts`, `mod:src/editor/core/cm`, `mod:src/editor/core/extras.ts`, `ipc:clipboard_file_list`, `unit:src/editor/core/cm/clipboard.test.ts`, `unit:src/editor/core/cm/docComment.test.ts`, `unit:src/editor/core/cm/docCommentTags.test.ts`, `unit:src/editor/core/cm/editing.test.ts`, `unit:src/editor/core/cm/ideaParity.test.ts`, `unit:src/editor/core/cm/indentGuides.test.ts`, `unit:src/editor/core/cm/lines.test.ts`, `unit:src/editor/core/cm/smartKeys.test.ts`, `unit:src/editor/core/cm/stringClipboard.test.ts`, `unit:src/editor/core/editingExtras.test.ts`, `unit:src/editor/core/view/PasteHistoryDialog.test.tsx`, `e2e:e2e/commentToggle.spec.ts`, `e2e:e2e/editor.spec.ts`.
- Spec files and cases (total cases at HEAD; 56 still to prove): `e2e/commentToggle.spec.ts` 1, `e2e/editor.spec.ts` 13, `src/editor/core/cm/clipboard.test.ts` 25, `src/editor/core/cm/docComment.test.ts` 8, `src/editor/core/cm/docCommentTags.test.ts` 14, `src/editor/core/cm/editing.test.ts` 48, `src/editor/core/cm/ideaParity.test.ts` 27, `src/editor/core/cm/indentGuides.test.ts` 18, `src/editor/core/cm/lines.test.ts` 5, `src/editor/core/cm/smartKeys.test.ts` 28, `src/editor/core/cm/stringClipboard.test.ts` 16, `src/editor/core/editingExtras.test.ts` 14, `src/editor/core/view/PasteHistoryDialog.test.tsx` 12.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 21848 lines, write about 3465; split into 3: smart keys + editing (76 cases), then clipboard + doc comments + the rest.
- Depends on: Q-VIS2 (Shift+Tab).

### 72. `F-RUN-CHOOSER` Run configuration chooser: All Configurations fold, Current File after, row ⋮ menu (Run, Debug, Edit, Pin, Delete), Unity defaults

- Webview sources (lines): `src/features/run/ConfigChooser.tsx` (518), `src/features/run/RunWidget.tsx` (243); spec files 922 lines.
- Native neighbours: `app-gpui/src/run/config_chooser.rs`, `app-gpui/src/run/chooser_view.rs`, `app-gpui/src/run/widget_view.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:run.attachUnity`, `act:run.withExecutor`, `ovl:run.chooser`, `title:run.widget`, `unit:src/features/run/chooserPinned.test.tsx`, `unit:src/features/run/RunWidget.test.tsx`, `e2e:e2e/runConfigDialogScroll.spec.ts`, `e2e:e2e/runConfigLayout.spec.ts`.
- Spec files and cases (total cases at HEAD; 22 still to prove): `e2e/runConfigDialogScroll.spec.ts` 1, `e2e/runConfigLayout.spec.ts` 7, `src/features/run/RunWidget.test.tsx` 23, `src/features/run/chooserPinned.test.tsx` 11.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 1683 lines, write about 962; one writer.
- Depends on: Q-FU2 (pin order item 2).

### 73. `F-NOTEBOOK` Notebook: cell docs, output view, security, session (kernel stand-in)

- Webview sources (lines): `src/features/notebook` (1902), `src/features/notebook/OutputView.tsx` (203), `src/features/notebook/security.ts` (285), `src/features/notebook/session.ts` (563); spec files 735 lines.
- Native neighbours: `app-gpui/src/notebook`, `app-gpui/src/features_a/notebook`, `app-gpui/src/workbench/notebook.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `kind:notebook`, `fep:notebook`, `pt:(at) src/features/notebook/index.ts:91`, `feat:notebook`, `ipc:notebook_cell_docs`, `unit:src/features/notebook/OutputView.test.tsx`, `unit:src/features/notebook/security.test.ts`, `unit:src/features/notebook/session.test.ts`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/features/notebook/OutputView.test.tsx` 9, `src/features/notebook/security.test.ts` 4, `src/features/notebook/session.test.ts` 18.
- Never in tests or the real window: TOOL (no Python kernel: notebook_execute is in C-TOOLCHAIN) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 3688 lines, write about 714; one writer.
- Depends on: none.

### 74. `I-WINDOW-GROUPS` Window tab groups (merge, detach, reorder, select, close together, restore at start)

- Webview sources (lines): `src/platform/windowGroups.ts` (55), `src/workbench/WindowGroupTabs.tsx` (123); spec files 82 lines.
- Native neighbours: `app-gpui/src/platform/window_groups.rs`, `app-gpui/src/app_models/windows.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:window.closeOtherTabs`, `act:window.closeTab`, `act:window.closeTabsToLeft`, `act:window.closeTabsToRight`, `act:window.detachTab`, `act:window.mergeAllWindows`, `act:window.nextTab`, `act:window.previousTab`, `act:window.reorderTabs`, `act:window.selectTab`, `ctx:windowGrouped`, `mod:src/platform/windowGroups.ts`, `mod:src/workbench/WindowGroupTabs.tsx`, `shell:src-tauri/src/window_groups.rs`, `ipc:window_group_cancel_close`, `ipc:window_group_close`, `ipc:window_group_close_tabs`, `ipc:window_group_current`, `ipc:window_group_detach`, `ipc:window_group_merge`, `ipc:window_group_reorder`, `ipc:window_group_restore`, `ipc:window_group_select`, `evt:WindowGroupsChanged`, `unit:src/platform/windowGroups.test.ts`, `unit:src/workbench/WindowGroupTabs.test.tsx`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/platform/windowGroups.test.ts` 2, `src/workbench/WindowGroupTabs.test.tsx` 3.
- Never in tests or the real window: WIN (WIN: the real window only through the inspection script, neutral paths).
- Size (Hypothesis): read about 260 lines, write about 2400; split into 2: model + actions first, then the tab strip and restore.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 92 %; Hypothesis).
- Depends on: none.

### 75. `F-PLAT-PROJECT-B` Platform project models: JDK, .NET, build model and recovery, page swipe, aux window, clipboard, IME, native drop

- Webview sources (lines): `src/platform/auxWindow.ts` (78), `src/platform/buildModel.ts` (16), `src/platform/buildToolRecovery.ts` (44), `src/platform/clipboard.ts` (68), `src/platform/clipboardSource.ts` (19), `src/platform/dotnet.ts` (285), `src/platform/ime.ts` (28), `src/platform/inspections.ts` (617), `src/platform/javaLaunch.ts` (269), `src/platform/jdk.ts` (68), `src/platform/nativeDrop.ts` (21), `src/platform/pageSwipe.ts` (38), `src/platform/pluginRunTargets.ts` (24), `src/platform/runTargetSelection.ts` (24); spec files 727 lines.
- Native neighbours: `app-gpui/src/platform_more/project.rs`, `app-gpui/src/platform/project_roots.rs`, `app-gpui/src/platform/virtual_docs.rs`, `app-gpui/src/platform_more/remote.rs`, `app-gpui/src/platform_more/dotnet.rs`, `app-gpui/src/platform_more/build_tool_recovery.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `mod:src/platform/auxWindow.ts`, `mod:src/platform/buildModel.ts`, `mod:src/platform/buildToolRecovery.ts`, `mod:src/platform/clipboard.ts`, `mod:src/platform/clipboardSource.ts`, `mod:src/platform/dotnet.ts`, `mod:src/platform/ime.ts`, `mod:src/platform/inspections.ts`, `mod:src/platform/javaLaunch.ts`, `mod:src/platform/jdk.ts`, `mod:src/platform/nativeDrop.ts`, `mod:src/platform/pageSwipe.ts`, `mod:src/platform/pluginRunTargets.ts`, `mod:src/platform/runTargetSelection.ts`, `ipc:clipboard_text`, `ipc:clipboard_write_text`, `unit:src/platform/auxWindow.test.ts`, `unit:src/platform/buildToolRecovery.test.ts`, `unit:src/platform/clipboard.test.ts`, `unit:src/platform/dotnet.test.ts`, `unit:src/platform/dotnetProcess.test.ts`, `unit:src/platform/ime.test.ts`, `unit:src/platform/jdk.test.ts`, `unit:src/platform/nativeDrop.test.ts`, `unit:src/platform/pageSwipe.test.ts`, `unit:src/workbench/buildToolRecovery.test.ts`.
- Spec files and cases (total cases at HEAD; 30 still to prove): `src/platform/auxWindow.test.ts` 3, `src/platform/buildToolRecovery.test.ts` 4, `src/platform/clipboard.test.ts` 6, `src/platform/dotnet.test.ts` 6, `src/platform/dotnetProcess.test.ts` 4, `src/platform/ime.test.ts` 6, `src/platform/jdk.test.ts` 4, `src/platform/nativeDrop.test.ts` 2, `src/platform/pageSwipe.test.ts` 9, `src/workbench/buildToolRecovery.test.ts` 2.
- Never in tests or the real window: HOME, TOOL (no JDK or dotnet is started: the lookup's process seam answers) (TOOL: no machine toolchain or real external program; HOME: nothing of the user's home).
- Size (Hypothesis): read about 2326 lines, write about 1999; split into 2: project/roots/virtual docs, then the rest.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 73 %; Hypothesis).
- Depends on: none.

### 76. `F-GRAPHQL` GraphQL: configurations, introspection, run query, environment, tool window

- Webview sources (lines): `src/features/graphql` (2278), `src/features/graphql/store.ts` (508); spec files 528 lines.
- Native neighbours: `app-gpui/src/features_a/graphql.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:graphql.addConfiguration`, `act:graphql.editConfiguration`, `act:graphql.introspect`, `act:graphql.restartDiscovery`, `act:graphql.runQuery`, `act:graphql.showToolWindow`, `pt:graphql.toolbar`, `ovl:graphql.environment`, `tw:graphql`, `feat:graphql`, `ipc:fs_create_file_bytes`, `unit:src/features/graphql/graphql.test.ts`, `unit:src/features/graphql/store.test.ts`.
- Spec files and cases (total cases at HEAD; 3 still to prove): `src/features/graphql/graphql.test.ts` 31, `src/features/graphql/store.test.ts` 2.
- Never in tests or the real window: NET (introspection against a loopback stand-in) (NET: no network).
- Size (Hypothesis): read about 3314 lines, write about 1425; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 85 %; Hypothesis).
- Depends on: none.

### 77. `F-RUN-IMPORT` Import run configurations (JetBrains, VS Code launch.json), run templates, context run producers

- Webview sources (lines): `src/features/run/importConfigs.tsx` (127), `src/features/run/producers.ts` (1384), `src/features/run/contextRun.ts` (418), `src/features/run/EditConfigsDialog.tsx` (746), `src/features/run/ConfigForm.tsx` (1135); spec files 2031 lines.
- Native neighbours: `app-gpui/src/run/edit_configs_view.rs`, `app-gpui/src/run_producers`, `app-gpui/src/run/configs.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:run.createConfig`, `act:run.editConfigs`, `act:run.importJetbrains`, `act:run.importVscodeLaunch`, `act:run.reloadConfigs`, `act:run.stop`, `ovl:run.dialogs`, `ovl:run.editConfigs`, `ipc:run_import_configs`, `ipc:run_import_sources`, `ipc:run_save_templates`, `unit:src/features/run/contextRun.test.ts`, `unit:src/features/run/importAction.test.tsx`, `unit:src/features/run/importConfigs.test.tsx`, `unit:src/features/run/producers.test.ts`.
- Spec files and cases (total cases at HEAD; 23 still to prove): `src/features/run/contextRun.test.ts` 16, `src/features/run/importAction.test.tsx` 3, `src/features/run/importConfigs.test.tsx` 7, `src/features/run/producers.test.ts` 67.
- Never in tests or the real window: HOME (imports read only the temp project's files) (HOME: nothing of the user's home).
- Size (Hypothesis): read about 5841 lines, write about 2437; split into 2: import, then producers (67 cases).
- Depends on: none.

### 78. `F-PL-VIEWS-B` Plugin views: web views (pty, files), HTML frame, plugin web shell module

- Webview sources (lines): see the spec files; spec files 1023 lines.
- Native neighbours: `app-gpui/src/plugin_views`, `app-gpui/src/workbench/plugin_tool_windows.rs`, `app-gpui/src/plugin_models/web_frames.rs`, `app-gpui/src/plugin_models/pty_bridge.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:(at) src/features/plugins/views/WebView.tsx:233`, `shell:src-tauri/src/plugin_web.rs`, `ipc:plugin_terminal_session_close`, `ipc:plugin_terminal_session_spawn`, `ipc:plugin_web_page`, `ipc:plugin_web_pty_spawn`, `ipc:plugin_web_read_file`, `ipc:plugin_web_save_file`, `unit:src/features/plugins/__tests__/htmlFrame.test.tsx`, `unit:src/features/plugins/__tests__/ptyBackend.test.ts`, `unit:src/features/plugins/__tests__/ptyBridge.test.ts`, `unit:src/features/plugins/__tests__/webViews.test.tsx`.
- Spec files and cases (total cases at HEAD; 19 still to prove): `src/features/plugins/__tests__/htmlFrame.test.tsx` 1, `src/features/plugins/__tests__/ptyBackend.test.ts` 1, `src/features/plugins/__tests__/ptyBridge.test.ts` 7, `src/features/plugins/__tests__/webViews.test.tsx` 18.
- Never in tests or the real window: TOOL (plugin web views' pty: a stand-in program, never a login shell), NET (web views load only files of the test plugin) (NET: no network; TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 1023 lines, write about 822; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 75 %; Hypothesis).
- Depends on: none.

### 79. `F-APP-SHELL-B` App shell: JetBrains settings import and updates

- Webview sources (lines): `src/features/app/jetbrainsSettings.tsx` (408), `src/features/app/updates.tsx` (286); spec files 1066 lines.
- Native neighbours: `app-gpui/src/app_models/close_guard.rs`, `app-gpui/src/app_models/windows.rs`, `app-gpui/src/app_models/shell.rs`, `app-gpui/src/app_models/cli_command.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:app.checkForUpdates`, `act:file.importJetBrainsSettings`, `shell:src-tauri/src/updates.rs`, `ipc:app_update_window_ready`, `ipc:settings_list_jetbrains_ides`, `ipc:settings_read_jetbrains`, `evt:UpdateStatusChanged`, `evt:UpdateAvailable`, `unit:src/features/app/__tests__/importFromJetBrainsIde.test.tsx`, `unit:src/features/app/__tests__/jetbrainsSettings.test.ts`, `unit:src/features/app/__tests__/jetbrainsSettingsImport.test.tsx`, `unit:src/features/app/__tests__/updates.test.ts`.
- Spec files and cases (total cases at HEAD; 19 still to prove): `src/features/app/__tests__/importFromJetBrainsIde.test.tsx` 5, `src/features/app/__tests__/jetbrainsSettings.test.ts` 9, `src/features/app/__tests__/jetbrainsSettingsImport.test.tsx` 1, `src/features/app/__tests__/updates.test.ts` 23.
- Never in tests or the real window: HOME (the CLI install writes only into a temp bin folder; Restart is checked to keep OPENIDEA_GPUI_HOME), NET (no update check) (NET: no network; HOME: nothing of the user's home).
- Size (Hypothesis): read about 1760 lines, write about 1184; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 83 %; Hypothesis).
- Depends on: Q-FU3 (closeWithoutWelcome).

### 80. `F-MACROS-RUNANY-SCRATCH` Macros (record, play, edit), Run Anything, scratch files

- Webview sources (lines): `src/features/macros` (593), `src/features/run-anything` (330), `src/features/scratch` (400), `src/features/macros/model.ts` (185), `src/features/run-anything/model.ts` (128); spec files 491 lines.
- Native neighbours: `app-gpui/src/features_b/macros.rs`, `app-gpui/src/features_b/run_anything.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:(dynamic) src/features/macros/index.ts:101`, `act:file.newScratchFile`, `act:macro.edit`, `act:macro.playBack`, `act:macro.playSaved`, `act:macro.startRecording`, `act:macro.stopRecording`, `act:runAnything.show`, `ctx:macroLastAvailable`, `ctx:macroRecording`, `ctx:macrosSaved`, `pt:macros.recorder`, `pt:scratch.new`, `ovl:macros.dialogs`, `ovl:runAnything.popup`, `ovl:scratch.chooser`, `pt:scratches`, `stat:macros.recording`, `feat:macros`, `feat:run-anything`, `feat:scratch`, `unit:src/features/macros/model.test.ts`, `unit:src/features/run-anything/model.test.ts`, `unit:src/features/scratch/__tests__/scratch.test.tsx`.
- Spec files and cases (total cases at HEAD; 14 still to prove): `src/features/macros/model.test.ts` 8, `src/features/run-anything/model.test.ts` 8, `src/features/scratch/__tests__/scratch.test.tsx` 10.
- Never in tests or the real window: TOOL (Run Anything runs a stand-in command only) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 2127 lines, write about 2227; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 92 %; Hypothesis).
- Depends on: none.

### 81. `F-PLAT-MODELS-B-A` Platform models II: fonts, themes, colour schemes, ICLS, CSS colours (models)

- Webview sources (lines): `src/platform/colorScheme.ts` (777), `src/platform/consoleFont.ts` (14), `src/platform/cssColor.ts` (78), `src/platform/fonts.ts` (224), `src/platform/icls.ts` (553), `src/platform/iconThemeOptions.ts` (58), `src/platform/systemFonts.ts` (72), `src/platform/themes.ts` (215), `src/platform/themeWatch.ts` (25); spec files 1366 lines.
- Native neighbours: `app-gpui/src/theme/fonts.rs`, `app-gpui/src/theme/color_scheme.rs`, `app-gpui/src/theme/icls.rs`, `app-gpui/src/theme/themes.rs`, `app-gpui/src/platform/css_color.rs`, `app-gpui/src/settings_view/pages` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `mod:src/platform/colorScheme.ts`, `mod:src/platform/consoleFont.ts`, `mod:src/platform/cssColor.ts`, `mod:src/platform/fonts.ts`, `mod:src/platform/icls.ts`, `mod:src/platform/iconThemeOptions.ts`, `mod:src/platform/systemFonts.ts`, `mod:src/platform/themes.ts`, `mod:src/platform/themeWatch.ts`, `unit:src/platform/colorScheme.test.ts`, `unit:src/platform/cssColor.test.ts`, `unit:src/platform/fonts.test.ts`, `unit:src/platform/icls.test.ts`, `unit:src/platform/themes.test.ts`.
- Spec files and cases (total cases at HEAD; 29 still to prove): `src/platform/colorScheme.test.ts` 22, `src/platform/cssColor.test.ts` 3, `src/platform/fonts.test.ts` 17, `src/platform/icls.test.ts` 31, `src/platform/themes.test.ts` 8.
- Never in tests or the real window: HOME (system font listing reads the machine's fonts: read only) (HOME: nothing of the user's home).
- Size (Hypothesis): read about 3382 lines, write about 1587; split into 2 by the native neighbours it touches.
- Depends on: none.

### 82. `F-LSP-POPUPS-A` Completion, hover, docs, signature, intentions, quick fixes, type info, hint, Popups, auto import and imports on paste, in-place rename, outside-project files: completion (55 cases) and its items

- Webview sources (lines): `src/editor/lsp/completion.ts` (935), `src/editor/lsp/completionItems.ts` (200), `src/editor/lsp/snippet.ts` (241); spec files 972 lines.
- Native neighbours: `app-gpui/src/editor_lsp/completion.rs`, `app-gpui/src/editor_lsp/hover.rs`, `app-gpui/src/editor_lsp/signature.rs`, `app-gpui/src/editor_lsp/intentions.rs`, `app-gpui/src/editor_lsp/inplace_rename.rs`, `app-gpui/src/editor_lsp/outside_project.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:code.completion`, `mod:src/editor/lsp/completion.ts`, `mod:src/editor/lsp/completionItems.ts`, `mod:src/editor/lsp/snippet.ts`, `ipc:lsp_execute_code_action`, `unit:src/editor/lsp/completion.test.ts`, `unit:src/editor/lsp/snippet.test.ts`.
- Spec files and cases (total cases at HEAD; 20 still to prove): `src/editor/lsp/completion.test.ts` 55, `src/editor/lsp/snippet.test.ts` 13.
- Never in tests or the real window: TOOL (stand-in language server) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 2348 lines, write about 1111; one writer.
- Depends on: Q-FU3, Q-FU4.

### 83. `F-EDITOR-DOCS-SYNC-A` Documents: disk sync, save on blur, custom and virtual documents

- Webview sources (lines): `src/editor/api.ts` (893), `src/editor/core/backend.ts` (718), `src/editor/core/diskSync.ts` (51), `src/editor/core/saveOnBlur.ts` (22), `src/editor/core/model/attachment.ts` (214), `src/editor/core/model/document.ts` (919), `src/editor/core/sync/DocSync.ts` (363); spec files 2319 lines.
- Native neighbours: `app-gpui/src/editor_model`, `app-gpui/src/editor/buffer.rs`, `app-gpui/src/platform/virtual_docs.rs`, `app-gpui/src/workbench/status_items.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:editor.document`, `mod:src/editor/api.ts`, `mod:src/editor/core/backend.ts`, `mod:src/editor/core/diskSync.ts`, `mod:src/editor/core/saveOnBlur.ts`, `ipc:doc_open_virtual`, `ipc:doc_sync_from_disk`, `ipc:lsp_virtual_document`, `unit:src/editor/core/customDocuments.test.ts`, `unit:src/editor/core/diskSync.test.ts`, `unit:src/editor/core/model/attachment.test.ts`, `unit:src/editor/core/model/document.test.ts`, `unit:src/editor/core/sync/DocSync.test.ts`, `unit:src/editor/core/virtual.test.ts`.
- Spec files and cases (total cases at HEAD; 37 still to prove): `src/editor/core/customDocuments.test.ts` 33, `src/editor/core/diskSync.test.ts` 7, `src/editor/core/model/attachment.test.ts` 10, `src/editor/core/model/document.test.ts` 22, `src/editor/core/sync/DocSync.test.ts` 17, `src/editor/core/virtual.test.ts` 5.
- Never in tests or the real window: NET (Download Sources: a stand-in; no Maven Central) (NET: no network).
- Size (Hypothesis): read about 5499 lines, write about 2259; split into 2: sync and virtual docs, then library sources.
- Depends on: Q-FU3 (encoding/BOM).

### 84. `F-AG-COMPOSER-TASKS-B` Agent composer and tasks: tasks: groups, pin, close, organize, project, dock, import sessions

- Webview sources (lines): `src/features/agents/organize.ts` (116); spec files 888 lines.
- Native neighbours: `app-gpui/src/agents/composer.rs`, `app-gpui/src/agents/task_list.rs`, `app-gpui/src/agents/new_task_view.rs`, `app-gpui/src/agents/task_actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:agents.closeAndNewTask`, `act:agents.closeTask`, `act:agents.dockTask`, `act:agents.importSessions`, `act:agents.newTaskGroup`, `act:agents.openSampleConversations`, `act:agents.openTask`, `act:agents.openTaskInEditor`, `act:agents.pinTasks`, `act:agents.removeTaskGroup`, `act:agents.renameTaskGroup`, `act:agents.undockTask`, `act:agents.unpinTasks`, `act:agents.useEditor`, `act:agents.useToolWindow`, `ovl:agents.importSessions`, `ipc:agent_external_sessions`, `ipc:agent_import_session`, `unit:src/features/agents/closeTask.test.ts`, `unit:src/features/agents/organize.test.ts`, `unit:src/features/agents/taskProject.test.ts`, `e2e:e2e/agentsDock.spec.ts`, `e2e:e2e/agentTaskProject.spec.ts`.
- Spec files and cases (total cases at HEAD; 29 still to prove): `e2e/agentTaskProject.spec.ts` 3, `e2e/agentsDock.spec.ts` 3, `src/features/agents/closeTask.test.ts` 2, `src/features/agents/organize.test.ts` 6, `src/features/agents/taskProject.test.ts` 15.
- Never in tests or the real window: AGENT (fake agent only; spawn_guard), HOME (external sessions are read from a temp folder only) (AGENT: never start a coding agent; HOME: nothing of the user's home).
- Size (Hypothesis): read about 1004 lines, write about 2062; split into 2: composer (folders 29, submission 17, commands), then task groups and organize, then changes panel.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 74 %; Hypothesis).
- Depends on: Q-FU2 (item 4).

### 85. `F-COVERAGE-PROFILER` Coverage (show, hide, import, manage reports, gutter) and profiler (CPU profiles, executors, live objects)

- Webview sources (lines): `src/features/coverage` (1024), `src/features/profiler` (802), `src/features/profiler/ProfilerView.tsx` (306), `src/features/profiler/cpuProfiles.ts` (46), `src/features/profiler/executor.ts` (158); spec files 596 lines.
- Native neighbours: `app-gpui/src/features_b/coverage.rs`, `app-gpui/src/features_b/profiler.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:coverage.hide`, `act:coverage.importReport`, `act:coverage.manageReports`, `act:coverage.show`, `act:profiler.showCpuProfiles`, `act:run.coverage`, `act:run.coverageContext`, `act:run.profile`, `act:run.profileAllocations`, `act:run.profileContext`, `act:run.profileLiveObjects`, `ctx:coverageSuites`, `ctx:coverageVisible`, `pt:coverage.gutter`, `ovl:coverage.manageReports`, `pt:(at) src/features/profiler/index.ts:15`, `pt:(at) src/features/profiler/index.ts:16`, `pt:(at) src/features/profiler/index.ts:17`, `pt:coverage`, `tw:coverage`, `tw:profiler`, `feat:coverage`, `feat:profiler`, `ipc:coverage_collect`, `ipc:coverage_import`, `ipc:coverage_prepare`, `ipc:profiler_collect`, `ipc:profiler_gc_before_stop`, `ipc:profiler_prepare`, `unit:src/features/coverage/coverage.test.ts`, `unit:src/features/profiler/cpuProfiles.test.ts`, `unit:src/features/profiler/executor.test.ts`, `unit:src/features/profiler/profiler.test.ts`, `unit:src/features/profiler/ProfilerView.test.tsx`.
- Spec files and cases (total cases at HEAD; 25 still to prove): `src/features/coverage/coverage.test.ts` 8, `src/features/profiler/ProfilerView.test.tsx` 8, `src/features/profiler/cpuProfiles.test.ts` 3, `src/features/profiler/executor.test.ts` 10, `src/features/profiler/profiler.test.ts` 5.
- Never in tests or the real window: TOOL (no profiler agent or coverage tool runs: recorded reports) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 2932 lines, write about 3179; split into 3 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 94 %; Hypothesis).
- Depends on: none.

### 86. `F-VCS-REMOTE-LINKS` Open on remote / Copy remote link, file web URLs, clone, create repository, worktrees

- Webview sources (lines): `src/features/vcs/actions.tsx` (780), `src/features/vcs/clone.tsx` (373), `src/features/vcs/createRepository.ts` (26), `src/features/vcs/worktrees.ts` (81); spec files 316 lines.
- Native neighbours: `app-gpui/src/vcs_ops/clone.rs`, `app-gpui/src/vcs_more/worktrees.rs`, `app-gpui/src/vcs_more/issue_links.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vcs.clone`, `act:vcs.copyRemoteLink`, `act:vcs.createRepository`, `act:vcs.openOnRemote`, `pt:vcs.openOnRemote`, `ipc:vcs_clone`, `ipc:vcs_init`, `ipc:vcs_move_worktree`, `unit:src/features/vcs/__tests__/copyRemoteLink.test.ts`, `unit:src/features/vcs/__tests__/fileWebUrl.test.ts`, `unit:src/features/vcs/worktrees.test.ts`.
- Spec files and cases (total cases at HEAD; 14 still to prove): `src/features/vcs/__tests__/copyRemoteLink.test.ts` 4, `src/features/vcs/__tests__/fileWebUrl.test.ts` 9, `src/features/vcs/worktrees.test.ts` 9.
- Never in tests or the real window: NET (clone only from a temp bare repository; no hosting service), GIT (NET: no network; GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 1576 lines, write about 1413; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 91 %; Hypothesis).
- Depends on: none.

### 87. `F-LSP-REFACTOR-A` Refactorings through the server: dialogs: Extract Interface, Change Signature, hierarchy (Pull Up / Push Down)

- Webview sources (lines): `src/editor/refactor/ChangeSignatureDialog.tsx` (207), `src/editor/refactor/extractInterface.ts` (178), `src/editor/refactor/ExtractInterfaceDialog.tsx` (142), `src/editor/refactor/hierarchy.ts` (207), `src/editor/refactor/HierarchyDialog.tsx` (147), `src/editor/refactor/signature.ts` (229); spec files 489 lines.
- Native neighbours: `app-gpui/src/editor_refactor`, `app-gpui/src/workbench/refactor_actions.rs`, `app-gpui/src/editor_lsp/workspace_edit.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:refactor.extractInterface`, `act:refactor.pullMembersUp`, `act:refactor.pushMembersDown`, `ovl:editor-lsp.changeSignatureDialog`, `ovl:editor-lsp.extractInterfaceDialog`, `ovl:editor-lsp.hierarchyDialog`, `mod:src/editor/refactor/ChangeSignatureDialog.tsx`, `mod:src/editor/refactor/extractInterface.ts`, `mod:src/editor/refactor/ExtractInterfaceDialog.tsx`, `mod:src/editor/refactor/hierarchy.ts`, `mod:src/editor/refactor/HierarchyDialog.tsx`, `mod:src/editor/refactor/signature.ts`, `ipc:lsp_change_signature`, `ipc:lsp_extract_interface`, `ipc:lsp_extract_interface_info`, `ipc:lsp_hierarchy_members`, `ipc:lsp_hierarchy_refactor`, `ipc:lsp_signature_info`, `unit:src/editor/refactor/extractInterface.test.ts`, `unit:src/editor/refactor/hierarchy.test.ts`, `unit:src/editor/refactor/signature.test.ts`, `e2e:e2e/hierarchyRefactor.spec.ts`.
- Spec files and cases (total cases at HEAD; 19 still to prove): `e2e/hierarchyRefactor.spec.ts` 3, `src/editor/refactor/extractInterface.test.ts` 3, `src/editor/refactor/hierarchy.test.ts` 10, `src/editor/refactor/signature.test.ts` 3.
- Never in tests or the real window: TOOL (stand-in language server answering the refactoring requests) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 1599 lines, write about 2170; split into 2: dialogs (Extract Interface, Change Signature, hierarchy) then the rest.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.

### 88. `F-RUN-TYPES-OTHER-C` Run types II: plugin attach and start, agent runs, container debug, plugin programs

- Webview sources (lines): `src/features/run/agentRuns.ts` (45), `src/features/run/containerDebug.ts` (92), `src/features/run/pluginAttach.ts` (38); spec files 147 lines.
- Native neighbours: `app-gpui/src/run/types.rs`, `app-gpui/src/run/executors.rs`, `app-gpui/src/run/go_actions.rs`, `app-gpui/src/run/targets.rs`, `app-gpui/src/run/csharp_targets.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:run.agentStart`, `act:run.pluginStart`, `ipc:mcp_agent_run_closed`, `ipc:plugin_answer_run_target`, `ipc:plugin_choose_program`, `ipc:plugin_debug_attach_target`, `ipc:plugin_program`, `ipc:plugin_run_targets`, `unit:src/features/run/agentRuns.test.ts`, `unit:src/features/run/containerDebug.test.ts`, `unit:src/features/run/pluginAttach.test.ts`.
- Spec files and cases (total cases at HEAD; 7 still to prove): `src/features/run/agentRuns.test.ts` 2, `src/features/run/containerDebug.test.ts` 2, `src/features/run/pluginAttach.test.ts` 4.
- Never in tests or the real window: TOOL (no dotnet, go, composer, bun, ruby: process seam), AGENT (agent runs: a recorded session only) (TOOL: no machine toolchain or real external program; AGENT: never start a coding agent).
- Size (Hypothesis): read about 322 lines, write about 787; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 91 %; Hypothesis).
- Depends on: none.

### 89. `F-DBG-JS-BROWSER` JavaScript browser debugging: Debug in Browser, browser sessions, Network tab, response overrides, page profile, JS custom breakpoints

- Webview sources (lines): `src/features/debugger/js` (1308), `src/features/debugger/browser.ts` (161); spec files 1166 lines.
- Native neighbours: `app-gpui/src/debugger/state.rs`, `app-gpui/src/debugger/launch.rs`, `app-gpui/src/debugger/actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:debug.debugInBrowser`, `act:debug.startConfig`, `act:debug.startPageProfile`, `act:debug.stopPageProfile`, `ctx:debugHasJsPage`, `ctx:debugPageProfiled`, `pt:js-browser`, `ipc:debug_js_custom_breakpoints`, `ipc:debug_js_response_body`, `ipc:debug_js_response_overrides`, `ipc:debug_js_start_profile`, `ipc:debug_js_stop_profile`, `ipc:web_browsers`, `ipc:web_serves_page`, `unit:src/features/debugger/__tests__/browser.test.ts`, `unit:src/features/debugger/__tests__/browserSession.test.ts`, `unit:src/features/debugger/__tests__/network.test.ts`, `unit:src/features/debugger/__tests__/networkSessions.test.ts`, `unit:src/features/debugger/__tests__/NetworkView.test.tsx`, `unit:src/features/debugger/__tests__/OverridesDialog.test.tsx`, `unit:src/features/debugger/__tests__/pageProfile.test.ts`, `unit:src/features/debugger/__tests__/responseOverrides.test.ts`.
- Spec files and cases (total cases at HEAD; 65 still to prove): `src/features/debugger/__tests__/NetworkView.test.tsx` 15, `src/features/debugger/__tests__/OverridesDialog.test.tsx` 6, `src/features/debugger/__tests__/browser.test.ts` 9, `src/features/debugger/__tests__/browserSession.test.ts` 8, `src/features/debugger/__tests__/network.test.ts` 10, `src/features/debugger/__tests__/networkSessions.test.ts` 2, `src/features/debugger/__tests__/pageProfile.test.ts` 6, `src/features/debugger/__tests__/responseOverrides.test.ts` 9.
- Never in tests or the real window: NET (no browser is launched; a stand-in CDP endpoint on loopback only), DEBUG (NET: no network; DEBUG: stand-in debug adapter only).
- Size (Hypothesis): read about 2635 lines, write about 2281; split into 2: sessions + Network tab, then overrides + profile.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 95 %; Hypothesis).
- Depends on: none.

### 90. `F-DBG-CONSOLE-WATCHES-A` Debug console (links, find, history, input to the program), expression completion, live watches, watch storage, event breakpoints, class imports, startup breakpoints: debug console: links, find, history, input, completion, process output, service console

- Webview sources (lines): see the spec files; spec files 543 lines.
- Native neighbours: `app-gpui/src/debugger/variables_view.rs`, `app-gpui/src/debugger/evaluate_dialog.rs`, `app-gpui/src/debugger/state.rs`, `app-gpui/src/debugger/breakpoints` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:debug.browseConsoleHistory`, `pt:(at) src/features/debugger/ui/DebugConsole.tsx:253`, `ipc:debug_completions`, `ipc:debug_send_input`, `ipc:tools_list`, `ipc:window_flash_taskbar`, `unit:src/features/debugger/__tests__/debugConsoleLinks.test.tsx`, `unit:src/features/debugger/__tests__/debugFind.test.tsx`, `unit:src/features/debugger/__tests__/expressionCompletion.test.tsx`, `unit:src/features/debugger/__tests__/processOutput.test.ts`, `unit:src/features/debugger/__tests__/serviceConsole.test.tsx`.
- Spec files and cases (total cases at HEAD; 23 still to prove): `src/features/debugger/__tests__/debugConsoleLinks.test.tsx` 4, `src/features/debugger/__tests__/debugFind.test.tsx` 10, `src/features/debugger/__tests__/expressionCompletion.test.tsx` 2, `src/features/debugger/__tests__/processOutput.test.ts` 3, `src/features/debugger/__tests__/serviceConsole.test.tsx` 4.
- Never in tests or the real window: DEBUG (stand-in adapter, python3 as today) (DEBUG: stand-in debug adapter only).
- Size (Hypothesis): read about 543 lines, write about 834; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.

### 91. `F-ANALYSIS` Code analysis: Inspect Code / File, inheritance inspections (Python), locate duplicates, cleanup, structural search and replace

- Webview sources (lines): `src/features/analysis` (2575), `src/features/analysis/inheritanceInspections.ts` (494), `src/features/analysis/inspect.ts` (261); spec files 751 lines.
- Native neighbours: `app-gpui/src/features_b/analysis` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:analyze.inspectCode`, `act:analyze.inspectFile`, `act:analyze.locateDuplicates`, `act:code.cleanup`, `act:find.replaceStructural`, `act:find.structural`, `pt:analysis.python-inheritance`, `ovl:analysis.dialogs`, `tw:duplicates`, `tw:inspection`, `feat:analysis`, `ipc:analysis_locate_duplicates`, `ipc:doc_python_facts`, `ipc:inspect_code`, `ipc:structural_languages`, `ipc:structural_search`, `ipc:structural_validate`, `unit:src/features/analysis/analysis.test.ts`, `unit:src/features/analysis/inheritanceInspections.test.ts`, `unit:src/features/analysis/inheritanceProfileRefresh.test.ts`, `unit:src/features/analysis/inspect.test.ts`, `unit:src/features/analysis/replacePreview.test.tsx`.
- Spec files and cases (total cases at HEAD; 29 still to prove): `src/features/analysis/analysis.test.ts` 8, `src/features/analysis/inheritanceInspections.test.ts` 23, `src/features/analysis/inheritanceProfileRefresh.test.ts` 1, `src/features/analysis/inspect.test.ts` 2, `src/features/analysis/replacePreview.test.tsx` 2.
- Never in tests or the real window: TOOL (no external linter runs) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 4081 lines, write about 2415; split into 2: inspections, then structural search.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 91 %; Hypothesis).
- Depends on: none.

### 92. `F-SMALL-FEATURES-B` Small features: libraries, colour preview, what's new, Cucumber, dependencies

- Webview sources (lines): `src/features/color-preview/colors.ts` (149), `src/features/cucumber/steps.ts` (475), `src/features/whats-new/whatsNew.tsx` (106); spec files 1009 lines.
- Native neighbours: `app-gpui/src/features_b`, `app-gpui/src/workbench/welcome.rs`, `app-gpui/src/features_a/todo.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:app.whatsNew`, `act:libraries.reload`, `pt:cucumber.undefinedSteps`, `pt:dependency-versions`, `kind:whatsNew`, `pt:(at) src/features/cucumber/index.ts:160`, `pt:externalLibraries`, `feat:color-preview`, `feat:cucumber`, `feat:dependencies`, `feat:libraries`, `feat:whats-new`, `ipc:app_release_notes`, `ipc:cucumber_sources`, `ipc:dependency_versions`, `ipc:sdk_project_libraries`, `unit:src/features/color-preview/colors.test.ts`, `unit:src/features/cucumber/steps.test.ts`, `unit:src/features/dependencies/dependencies.test.ts`, `unit:src/features/libraries/libraries.test.ts`, `unit:src/features/libraries/refresh.test.ts`, `unit:src/features/whats-new/__tests__/whatsNew.test.tsx`.
- Spec files and cases (total cases at HEAD; 37 still to prove): `src/features/color-preview/colors.test.ts` 7, `src/features/cucumber/steps.test.ts` 14, `src/features/dependencies/dependencies.test.ts` 11, `src/features/libraries/libraries.test.ts` 8, `src/features/libraries/refresh.test.ts` 1, `src/features/whats-new/__tests__/whatsNew.test.tsx` 17.
- Never in tests or the real window: NET (web preview only on loopback; What's New reads a fixture), TOOL (NET: no network; TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 1739 lines, write about 1761; split into 2: one writer per two or three features.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 86 %; Hypothesis).
- Depends on: none.

### 93. `F-UI-PRIMITIVES` Design-system primitives: Tree (+reorder), Tabs, Select, Dialog, List, Popup, FloatingToolbar, Toolbar, Splitter, Scrollbar, Mnemonic, Button, LogPane, SourcePreview, focus scope, selection boundary, drag cursor, icons

- Webview sources (lines): `src/ui/` (8967), `src/ui/ansi.ts` (160), `src/ui/BarButton.tsx` (34), `src/ui/Button.tsx` (103), `src/ui/Checkbox.tsx` (121), `src/ui/Chip.tsx` (24), `src/ui/Dialog.tsx` (521), `src/ui/dragCursor.ts` (50), `src/ui/Feedback.tsx` (148), `src/ui/filePresentation.ts` (192), `src/ui/FloatingToolbar.tsx` (81), `src/ui/focusScope.ts` (33), `src/ui/HighlightedText.tsx` (42), `src/ui/iconAppearance.ts` (69), `src/ui/iconNode.ts` (10), `src/ui/icons.tsx` (1305), `src/ui/index.ts` (47), `src/ui/Input.tsx` (290), `src/ui/Kbd.tsx` (20), `src/ui/lazy.tsx` (79), `src/ui/List.tsx` (266), `src/ui/LogPane.tsx` (155), `src/ui/match.ts` (71), `src/ui/Menu.tsx` (579), `src/ui/menuAim.ts` (20), `src/ui/Mnemonic.tsx` (34), `src/ui/patternIndex.ts` (219), `src/ui/pointerDrag.ts` (234), `src/ui/Popup.tsx` (267), `src/ui/popupState.ts` (120), `src/ui/Scrollbar.tsx` (111), `src/ui/Select.tsx` (361), `src/ui/selectableText.ts` (103), `src/ui/SourcePreview.tsx` (244), `src/ui/Spinner.tsx` (46), `src/ui/Splitter.tsx` (119), `src/ui/svgSize.ts` (147), `src/ui/symbolKind.ts` (20), `src/ui/T.tsx` (11), `src/ui/Tabs.tsx` (571), `src/ui/tokenSpans.tsx` (39), `src/ui/Toolbar.tsx` (88), `src/ui/Tooltip.tsx` (145), `src/ui/Tree.tsx` (921), `src/ui/treeDirectoryContext.ts` (36), `src/ui/treeReorder.ts` (135), `src/ui/useSanitizedField.ts` (34), `src/ui/useSelectionBoundary.ts` (129); spec files 4219 lines.
- Native neighbours: `app-gpui/src/ui`, `app-gpui/src/theme/icons.rs`, `app-gpui/src/platform_more/selectable_text.rs`, `app-gpui/src/platform_more/pointer_drag.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: 76 rows (47 mod, 26 unit, 1 ctx, 1 pt, 1 ipc); first 20: `ctx:focusedDirectory`, `pt:(at) src/ui/Tree.tsx:389`, `mod:src/ui/ansi.ts`, `mod:src/ui/BarButton.tsx`, `mod:src/ui/Button.tsx`, `mod:src/ui/Checkbox.tsx`, `mod:src/ui/Chip.tsx`, `mod:src/ui/Dialog.tsx`, `mod:src/ui/dragCursor.ts`, `mod:src/ui/Feedback.tsx`, `mod:src/ui/filePresentation.ts`, `mod:src/ui/FloatingToolbar.tsx`, `mod:src/ui/focusScope.ts`, `mod:src/ui/HighlightedText.tsx`, `mod:src/ui/iconAppearance.ts`, `mod:src/ui/iconNode.ts`, `mod:src/ui/icons.tsx`, `mod:src/ui/index.ts`, `mod:src/ui/Input.tsx`, `mod:src/ui/Kbd.tsx`; all in `assignment.tsv` under `F-UI-PRIMITIVES`.
- Spec files and cases (total cases at HEAD; 199 still to prove): `src/ui/Button.test.tsx` 3, `src/ui/Dialog.test.tsx` 22, `src/ui/FloatingToolbar.test.tsx` 5, `src/ui/List.test.tsx` 9, `src/ui/LogPane.test.tsx` 6, `src/ui/Mnemonic.test.tsx` 2, `src/ui/Popup.test.tsx` 23, `src/ui/Scrollbar.test.tsx` 1, `src/ui/Select.test.tsx` 14, `src/ui/SourcePreview.test.tsx` 7, `src/ui/Splitter.test.tsx` 3, `src/ui/Tabs.test.tsx` 23, `src/ui/Toolbar.test.tsx` 3, `src/ui/Tree.reorder.test.tsx` 8, `src/ui/Tree.test.tsx` 37, `src/ui/dragCursor.test.ts` 4, `src/ui/filePresentation.cache.test.tsx` 5, `src/ui/filePresentation.test.ts` 6, `src/ui/focusScope.test.ts` 3, `src/ui/iconAppearance.test.tsx` 2, `src/ui/icons.test.tsx` 16, `src/ui/menuAim.test.ts` 4, `src/ui/pointerDrag.test.ts` 8, `src/ui/selectableSurfaces.test.tsx` 4, `src/ui/selectableText.test.ts` 13, `src/ui/useSelectionBoundary.test.tsx` 10.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 21740 lines, write about 9694; split into 7: Tree + reorder (45 cases), Tabs + Select + Dialog (59), the rest (one writer each).
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 78 %; Hypothesis).
- Depends on: none.
- Note: Most of these are proven through the native views that use the primitive; a primitive row closes when its spec's cases are asserted on a native view.

### 94. `F-DB-TREE-SOURCES-B` Database tree and sources: sources (dialog, connect, SSH tunnel secret), export, schema compare, import

- Webview sources (lines): `src/features/database/ExportDialog.tsx` (179), `src/features/database/SourceDialog.tsx` (1279), `src/features/database/export.ts` (117), `src/features/database/schemaCompare.ts` (234); spec files 565 lines.
- Native neighbours: `app-gpui/src/database/tree_view.rs`, `app-gpui/src/database/source_dialog.rs`, `app-gpui/src/database/export.rs`, `app-gpui/src/database/objects.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:db.compareSchemas`, `act:db.exportData`, `act:db.importData`, `act:db.pluginConnect`, `ovl:database.dialogs`, `ipc:db_connect`, `ipc:db_set_ssh_secret`, `ipc:db_trust_ssh_host`, `unit:src/features/database/export.test.ts`, `unit:src/features/database/ExportDialog.test.tsx`, `unit:src/features/database/schemaCompare.test.ts`, `unit:src/features/database/SourceDialog.test.tsx`, `e2e:e2e/databaseJsonExport.spec.ts`.
- Spec files and cases (total cases at HEAD; 25 still to prove): `e2e/databaseJsonExport.spec.ts` 3, `src/features/database/ExportDialog.test.tsx` 3, `src/features/database/SourceDialog.test.tsx` 4, `src/features/database/export.test.ts` 16, `src/features/database/schemaCompare.test.ts` 2.
- Never in tests or the real window: TOOL (SQLite), NET (no server or SSH tunnel; tunnel test against a refused loopback port), SECRET (passwords and SSH secrets) (NET: no network; TOOL: no machine toolchain or real external program; SECRET: secret values never in Debug, logs or failure text).
- Size (Hypothesis): read about 2374 lines, write about 1683; split into 2: introspection + objects, then export + compare + sources.
- Depends on: none.

### 95. `F-APP-SHELL-A` App shell: close guard and windows

- Webview sources (lines): `src/features/app/closeGuard.ts` (383), `src/features/app/windows.ts` (637); spec files 1257 lines.
- Native neighbours: `app-gpui/src/app_models/close_guard.rs`, `app-gpui/src/app_models/windows.rs`, `app-gpui/src/app_models/shell.rs`, `app-gpui/src/app_models/cli_command.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:app.restart`, `ovl:app.termination`, `shell:src-tauri/src/application_focus.rs`, `shell:src-tauri/src/window_work_area.rs`, `shell:src-tauri/src/windows.rs`, `ipc:app_quit`, `ipc:app_restart`, `ipc:window_close_context`, `ipc:window_complete_launch`, `ipc:window_open_project`, `unit:src/features/app/__tests__/closeGuard.test.ts`, `unit:src/features/app/__tests__/windowClose.test.ts`, `unit:src/features/app/__tests__/windows.test.ts`.
- Spec files and cases (total cases at HEAD; 46 still to prove): `src/features/app/__tests__/closeGuard.test.ts` 25, `src/features/app/__tests__/windowClose.test.ts` 7, `src/features/app/__tests__/windows.test.ts` 52.
- Never in tests or the real window: HOME (the CLI install writes only into a temp bin folder; Restart is checked to keep OPENIDEA_GPUI_HOME), NET (no update check) (NET: no network; HOME: nothing of the user's home).
- Size (Hypothesis): read about 2277 lines, write about 1575; split into 2: close guard + windows (77 cases), then shell/menu/CLI, then JetBrains import.
- Depends on: Q-FU3 (closeWithoutWelcome) (slices: I-UNTITLED).

### 96. `F-EDITOR-EMBEDDED-LANGS` Embedded languages in the editor: Angular, Astro, Django, JSP, ERB, Vue, Thymeleaf, Helm, go.mod, Gherkin, shell interpolation, linked tags

- Webview sources (lines): `src/editor/core/embeddedDocuments.ts` (49), `src/editor/lineTokens.tsx` (142), `src/editor/core/cm/angular/inline.ts` (47), `src/editor/core/cm/cmake.ts` (159), `src/editor/core/cm/erb.ts` (78), `src/editor/core/cm/gherkin.ts` (51), `src/editor/core/cm/gomod.ts` (111), `src/editor/core/cm/helm.ts` (296), `src/editor/core/cm/interpolation.ts` (158), `src/editor/core/cm/languages.ts` (462), `src/editor/core/cm/linkedTags.ts` (84), `src/editor/core/cm/shellInterpolation.ts` (148), `src/editor/core/cm/syntaxHighlight.ts` (69), `src/editor/core/cm/thymeleaf.ts` (313), `src/editor/core/cm/vue.ts` (27), `src/features/web/djangoTemplates.ts` (91), `src/test/syntax.ts` (17); spec files 3234 lines.
- Native neighbours: `app-gpui/src/editor/language_facts.rs`, `app-gpui/src/editor_model`, `app-gpui/src/editor/intelligence.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:web.djangoTemplate`, `pt:web.djangoContent`, `pt:vscode-notebook-cell`, `mod:src/editor/core/embeddedDocuments.ts`, `mod:src/editor/lineTokens.tsx`, `unit:src/editor/core/cm/angular/angular.test.ts`, `unit:src/editor/core/cm/angular/inline.test.ts`, `unit:src/editor/core/cm/angularLoading.test.ts`, `unit:src/editor/core/cm/astro/astro.test.ts`, `unit:src/editor/core/cm/cmake.test.ts`, `unit:src/editor/core/cm/django/django.test.ts`, `unit:src/editor/core/cm/erb.test.ts`, `unit:src/editor/core/cm/gherkin.test.ts`, `unit:src/editor/core/cm/gomod.test.ts`, `unit:src/editor/core/cm/helm.test.ts`, `unit:src/editor/core/cm/interpolation.test.ts`, `unit:src/editor/core/cm/interpolationIncremental.test.ts`, `unit:src/editor/core/cm/jsp/jsp.test.ts`, `unit:src/editor/core/cm/languages.test.ts`, `unit:src/editor/core/cm/linkedTags.test.ts`, `unit:src/editor/core/cm/shellInterpolation.test.ts`, `unit:src/editor/core/cm/syntaxHighlight.test.ts`, `unit:src/editor/core/cm/thymeleaf.test.ts`, `unit:src/editor/core/cm/vue.test.ts`, `unit:src/editor/core/embeddedDocuments.test.ts`, `unit:src/editor/core/model/documentLanguage.test.ts`, `unit:src/editor/lazyEditorExtension.test.ts`, `unit:src/editor/lineTokens.test.ts`, `unit:src/features/web/djangoTemplates.test.ts`, `unit:src/test/syntax.test.ts`, `e2e:e2e/angularInline.spec.ts`, `e2e:e2e/httpHighlight.spec.ts`.
- Spec files and cases (total cases at HEAD; 155 still to prove): `e2e/angularInline.spec.ts` 1, `e2e/httpHighlight.spec.ts` 1, `src/editor/core/cm/angular/angular.test.ts` 24, `src/editor/core/cm/angular/inline.test.ts` 8, `src/editor/core/cm/angularLoading.test.ts` 1, `src/editor/core/cm/astro/astro.test.ts` 7, `src/editor/core/cm/cmake.test.ts` 7, `src/editor/core/cm/django/django.test.ts` 15, `src/editor/core/cm/erb.test.ts` 2, `src/editor/core/cm/gherkin.test.ts` 1, `src/editor/core/cm/gomod.test.ts` 4, `src/editor/core/cm/helm.test.ts` 3, `src/editor/core/cm/interpolation.test.ts` 9, `src/editor/core/cm/interpolationIncremental.test.ts` 2, `src/editor/core/cm/jsp/jsp.test.ts` 14, `src/editor/core/cm/languages.test.ts` 11, `src/editor/core/cm/linkedTags.test.ts` 22, `src/editor/core/cm/shellInterpolation.test.ts` 6, `src/editor/core/cm/syntaxHighlight.test.ts` 3, `src/editor/core/cm/thymeleaf.test.ts` 4, `src/editor/core/cm/vue.test.ts` 2, `src/editor/core/embeddedDocuments.test.ts` 1, `src/editor/core/model/documentLanguage.test.ts` 1, `src/editor/lazyEditorExtension.test.ts` 1, `src/editor/lineTokens.test.ts` 6, `src/features/web/djangoTemplates.test.ts` 2, `src/test/syntax.test.ts` 3.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 5536 lines, write about 3895; split into 3: one writer per two or three languages (Angular + Astro, Django + JSP + ERB, Vue + Thymeleaf + Helm + small ones, linked tags).
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 91 %; Hypothesis).
- Depends on: none.

### 97. `F-RUN-TYPES-JVM` Run types I: Java / JVM module, Gradle run, Tomcat, JAR application, Java debug, remote JVM, application build/publish/update

- Webview sources (lines): `src/features/run/jvmTypes.ts` (288), `src/features/run/tomcat.ts` (187), `src/features/run/gradleRun.ts` (194), `src/features/run/javaDebug.ts` (237), `src/features/run/applicationUpdate.ts` (157); spec files 2071 lines.
- Native neighbours: `app-gpui/src/run/jvm_types.rs`, `app-gpui/src/run/java_launch.rs`, `app-gpui/src/run/gradle_run.rs`, `app-gpui/src/run/java_debug.rs`, `app-gpui/src/run/application_update.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:run.updateApplication`, `pt:jar-application`, `pt:tomcat`, `ipc:debug_free_port`, `ipc:java_build_workspace`, `ipc:java_debug_server`, `ipc:java_resolve_launch`, `ipc:run_application_build`, `ipc:run_application_publish`, `ipc:run_tomcat_artifacts`, `ipc:run_tomcat_build`, `ipc:run_tomcat_home`, `ipc:run_tomcat_installations`, `ipc:run_tomcat_sync`, `ipc:run_tomcat_update`, `unit:src/features/run/gradleRun.test.ts`, `unit:src/features/run/java.test.ts`, `unit:src/features/run/javaDebug.test.ts`, `unit:src/features/run/javaModule.test.ts`, `unit:src/features/run/jvmTypes.test.ts`, `unit:src/features/run/tomcat.test.tsx`.
- Spec files and cases (total cases at HEAD; 43 still to prove): `src/features/run/gradleRun.test.ts` 19, `src/features/run/java.test.ts` 42, `src/features/run/javaDebug.test.ts` 3, `src/features/run/javaModule.test.ts` 19, `src/features/run/jvmTypes.test.ts` 22, `src/features/run/tomcat.test.tsx` 14.
- Never in tests or the real window: TOOL (no JDK, Gradle or Tomcat runs: process seam and recorded answers) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 3134 lines, write about 2076; split into 2: Java/JVM module + Gradle, then Tomcat + application.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 76 %; Hypothesis).
- Depends on: none.

### 98. `F-SVC-DOCKER-A` Services: containers and images: actions, logs, inspect, processes, layers

- Webview sources (lines): `src/features/services/` (6318); spec files 0 lines.
- Native neighbours: `app-gpui/src/services`, `app-gpui/src/workbench/services_window.rs`, `app-gpui/src/workbench/services_actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:delete`, `act:docker.execShell`, `act:docker.inspect`, `act:docker.openLogInEditor`, `act:docker.prune`, `act:docker.pullImage`, `act:docker.remove`, `act:docker.restart`, `act:docker.showLayers`, `act:docker.showLog`, `act:docker.showProcesses`, `act:docker.start`, `act:docker.stop`, `act:pull`, `act:refresh`, `act:restart`, `act:start`, `act:terminal`, `kind:dockerLog`, `pt:(at) src/features/services/logEditor.tsx:273`, `pt:docker-image`.
- Never in tests or the real window: DOCKER (never the machine's daemon: a stand-in Docker API on a temp socket; the daemon rows' parity rests on it), NET (no registry login or pull) (NET: no network; DOCKER: never the Docker daemon).
- Size (Hypothesis): read about 6318 lines, write about 2715; split into 2: containers + actions, then Compose + gutter, then dashboard.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.
- Note: The docker_* commands are in C-TOOLCHAIN; this slice proves the UI against a stand-in.

### 99. `F-AG-COMPOSER-TASKS-A` Agent composer and tasks: composer: folders, submission, commands, draft and click e2e

- Webview sources (lines): see the spec files; spec files 1690 lines.
- Native neighbours: `app-gpui/src/agents/composer.rs`, `app-gpui/src/agents/task_list.rs`, `app-gpui/src/agents/new_task_view.rs`, `app-gpui/src/agents/task_actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:agents.send`, `ipc:clipboard_image`, `ipc:find_dirs`, `ipc:find_symbols_named`, `ipc:path_completions`, `unit:src/features/agents/composerCommands.test.tsx`, `unit:src/features/agents/composerFolders.test.tsx`, `unit:src/features/agents/composerSubmission.test.tsx`, `e2e:e2e/agentComposerClick.spec.ts`, `e2e:e2e/agentDraftSubmission.spec.ts`.
- Spec files and cases (total cases at HEAD; 49 still to prove): `e2e/agentComposerClick.spec.ts` 1, `e2e/agentDraftSubmission.spec.ts` 1, `src/features/agents/composerCommands.test.tsx` 21, `src/features/agents/composerFolders.test.tsx` 29, `src/features/agents/composerSubmission.test.tsx` 17.
- Never in tests or the real window: AGENT (fake agent only; spawn_guard), HOME (external sessions are read from a temp folder only) (AGENT: never start a coding agent; HOME: nothing of the user's home).
- Size (Hypothesis): read about 1690 lines, write about 1152; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 80 %; Hypothesis).
- Depends on: Q-FU2 (item 4).

### 100. `F-PL-VIEWS-A` Plugin views: content editor, editor panels, view windows, logs, projection, project UI

- Webview sources (lines): see the spec files; spec files 2226 lines.
- Native neighbours: `app-gpui/src/plugin_views`, `app-gpui/src/workbench/plugin_tool_windows.rs`, `app-gpui/src/plugin_models/web_frames.rs`, `app-gpui/src/plugin_models/pty_bridge.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:plugins.content.action`, `act:plugins.content.find`, `act:plugins.log.find`, `act:plugins.log.wrap`, `pt:plugins.editorPanels`, `pt:(at) src/features/plugins/views/editorPanel.tsx:304`, `kind:pluginContent`, `kind:pluginEditor`, `kind:pluginLog`, `pt:(at) src/features/plugins/views/contentEditor.tsx:521`, `pt:(at) src/features/plugins/views/logEditor.tsx:241`, `tw:(at) src/features/plugins/views/viewWindows.tsx:118`, `unit:src/features/plugins/__tests__/contentEditor.test.tsx`, `unit:src/features/plugins/__tests__/editorPanels.test.tsx`, `unit:src/features/plugins/__tests__/logEditor.test.tsx`, `unit:src/features/plugins/__tests__/projectUi.test.tsx`, `unit:src/features/plugins/__tests__/verbatimText.test.tsx`, `unit:src/features/plugins/__tests__/viewProjection.test.tsx`, `unit:src/features/plugins/__tests__/views.test.tsx`, `unit:src/features/plugins/__tests__/viewWindows.test.tsx`.
- Spec files and cases (total cases at HEAD; 64 still to prove): `src/features/plugins/__tests__/contentEditor.test.tsx` 13, `src/features/plugins/__tests__/editorPanels.test.tsx` 7, `src/features/plugins/__tests__/logEditor.test.tsx` 8, `src/features/plugins/__tests__/projectUi.test.tsx` 5, `src/features/plugins/__tests__/verbatimText.test.tsx` 4, `src/features/plugins/__tests__/viewProjection.test.tsx` 6, `src/features/plugins/__tests__/viewWindows.test.tsx` 12, `src/features/plugins/__tests__/views.test.tsx` 26.
- Never in tests or the real window: TOOL (plugin web views' pty: a stand-in program, never a login shell), NET (web views load only files of the test plugin) (NET: no network; TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 2226 lines, write about 2172; split into 2: content editor + panels + windows, then web views + pty, then service provider.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 80 %; Hypothesis).
- Depends on: none.

### 101. `F-PL-VIEWS-C` Plugin views: service pane and provider, text editor wrappers, file editor providers

- Webview sources (lines): see the spec files; spec files 365 lines.
- Native neighbours: `app-gpui/src/plugin_views`, `app-gpui/src/workbench/plugin_tool_windows.rs`, `app-gpui/src/plugin_models/web_frames.rs`, `app-gpui/src/plugin_models/pty_bridge.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:(dynamic) src/features/plugins/views/serviceProvider.tsx:88`, `fep:(at) src/features/plugins/uiPoints.tsx:484`, `pt:(at) src/features/plugins/uiPoints.tsx:379`, `pt:(at) src/features/plugins/uiPoints.tsx:478`, `ipc:plugin_content_tab_action`, `ipc:plugin_service_detail`, `ipc:plugin_service_log`, `unit:src/features/plugins/__tests__/servicePane.test.tsx`, `unit:src/features/plugins/__tests__/serviceProvider.test.tsx`, `e2e:e2e/pluginViews.spec.ts`.
- Spec files and cases (total cases at HEAD; 15 still to prove): `e2e/pluginViews.spec.ts` 1, `src/features/plugins/__tests__/servicePane.test.tsx` 4, `src/features/plugins/__tests__/serviceProvider.test.tsx` 10.
- Never in tests or the real window: TOOL (plugin web views' pty: a stand-in program, never a login shell), NET (web views load only files of the test plugin) (NET: no network; TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 365 lines, write about 810; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.

### 102. `F-WIT` Plugin API interfaces (wit/): one test per interface through a test plugin, after the feature slice that surfaces it

- Webview sources (lines): `wit` (0); spec files 0 lines.
- Native neighbours: `app-gpui/src/plugins.rs`, `app-gpui/src/workbench/plugin_runtime.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: 46 rows (46 wit); first 20: `wit:commit`, `wit:commit-checker`, `wit:database`, `wit:decorations`, `wit:detail-types`, `wit:detail-provider`, `wit:diagnostics-provider`, `wit:endpoints-provider`, `wit:events`, `wit:tasks`, `wit:lifecycle`, `wit:syntax-provider`, `wit:run-target-validation`, `wit:extensions`, `wit:file-actions`, `wit:fs`, `wit:log`, `wit:ui`, `wit:storage`, `wit:language-servers`; all in `assignment.tsv` under `F-WIT`.
- Never in tests or the real window: the test plugins are built from fixtures already in the repository; rebuilding a wasm fixture is a change under crates/ (the manager's call).
- Size (Hypothesis): read about 0 lines, write about 2300; split into 2: one integration per five or six interfaces; most interfaces close with the feature slice that surfaces them.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 72 %; Hypothesis).
- Depends on: F-PL-VIEWS, F-PL-NAV-EVENTS, F-VCS-COMMIT-DIALOG (commit, commit-checker), F-DB-TREE-SOURCES (database) (slices: F-DB-TREE-SOURCES-A, F-DB-TREE-SOURCES-B, F-PL-NAV-EVENTS, F-PL-VIEWS-A, F-PL-VIEWS-B, F-PL-VIEWS-C, F-VCS-COMMIT-DIALOG).

### 103. `F-BT-POM` POM completion and navigation, version catalog, published catalogs

- Webview sources (lines): `src/features/build-tools/pomCompletion.ts` (51), `src/features/build-tools/pomNavigation.ts` (91), `src/features/build-tools/versionCatalog.ts` (266); spec files 413 lines.
- Native neighbours: `app-gpui/src/build_tools`, `app-gpui/src/editor_lsp/completion.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:build-tools.pomCompletion`, `pt:build-tools.pomNavigation`, `pt:build-tools.versionCatalog`, `ipc:gradle_published_catalog`, `ipc:maven_dependency_completion`, `ipc:maven_dependency_declaration`, `ipc:maven_goto_declaration`, `unit:src/features/build-tools/pomCompletion.test.ts`, `unit:src/features/build-tools/pomNavigation.test.ts`, `unit:src/features/build-tools/versionCatalog.test.ts`.
- Spec files and cases (total cases at HEAD; 15 still to prove): `src/features/build-tools/pomCompletion.test.ts` 3, `src/features/build-tools/pomNavigation.test.ts` 7, `src/features/build-tools/versionCatalog.test.ts` 5.
- Never in tests or the real window: NET (no Maven Central: a stand-in index), HOME (NET: no network; HOME: nothing of the user's home).
- Size (Hypothesis): read about 821 lines, write about 832; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.

### 104. `F-TEMPLATES-REST` Templates: Emmet expansion (Tab, Django rule M8), templates in the completion list, Expressions chooser, plugin snippets, the feature row

- Webview sources (lines): `src/features/templates/emmet` (336), `src/features/templates/editor.ts` (633); spec files 80 lines.
- Native neighbours: `app-gpui/src/templates`, `app-gpui/src/workbench/emmet.rs`, `app-gpui/src/workbench/editor_templates.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `ctx:EDITOR_WHEN`, `pt:templates`, `set:editor.fileTemplates`, `set:editor.liveTemplates`, `feat:templates`, `unit:src/features/templates/__tests__/emmet.test.ts`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/features/templates/__tests__/emmet.test.ts` 8.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 1049 lines, write about 460; one writer.
- Depends on: Q-NFT, T1-TEMPLATES-UP (slices: T1-TEMPLATES-UP).

### 105. `F-AG-MODELS` Agents' model catalog: ModelCatalog event, Refresh Models, cost and usage badges, Reset, effort picker cycling, pinned models

- Webview sources (lines): `src/features/agents/ConfigPickers.tsx` (563), `src/features/agents/store.ts` (1562); spec files 1337 lines.
- Native neighbours: `app-gpui/src/agents/config_pickers.rs`, `app-gpui/src/agents/store.rs`, `app-gpui/src/agents/config_bar.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:agents.cycleEffort`, `act:agents.refreshModels`, `ctx:agentEffortAvailable`, `ipc:agent_fork`, `ipc:agent_providers`, `ipc:agent_refresh_models`, `ipc:agent_subscribe`, `unit:src/features/agents/ConfigPickers.test.tsx`, `unit:src/features/agents/modelRefresh.test.tsx`, `e2e:e2e/agentCycleEffort.spec.ts`, `e2e:e2e/agentVersion.spec.ts`.
- Spec files and cases (total cases at HEAD; 30 still to prove): `e2e/agentCycleEffort.spec.ts` 12, `e2e/agentVersion.spec.ts` 2, `src/features/agents/ConfigPickers.test.tsx` 16, `src/features/agents/modelRefresh.test.tsx` 11.
- Never in tests or the real window: AGENT (a fake agent declared in the test; spawn_guard stays), SECRET (provider keys) (AGENT: never start a coding agent; SECRET: secret values never in Debug, logs or failure text).
- Size (Hypothesis): read about 3462 lines, write about 1733; split into 2 by the native neighbours it touches.
- Depends on: Q-AGL.

### 106. `I-SYSNOTIFY` System notifications while the app is in the background (setting, chime rule)

- Webview sources (lines): `src/workbench/notificationStore.ts` (223), `src/workbench/messages.tsx` (160), `src/workbench/Notifications.tsx` (186); spec files 87 lines.
- Native neighbours: `app-gpui/src/workbench/notifications.rs`, `app-gpui/src/settings_model/builtin_pages/behavior.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `tw:notifications`, `set:notifications`, `mod:src/workbench/messages.tsx`, `mod:src/workbench/Notifications.tsx`, `mod:src/workbench/notificationStore.ts`, `shell:src-tauri/src/system_notifications.rs`, `ipc:show_system_notification`, `evt:Notify`, `unit:src/workbench/messages.test.tsx`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/workbench/messages.test.tsx` 4.
- Never in tests or the real window: tests through a recording seam; the real OS notification is seen once by the inspection script only.
- Size (Hypothesis): read about 656 lines, write about 631; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 56 %; Hypothesis).
- Depends on: none.
- Note: Hypothesis: GPUI has no notification API; the macOS call is platform code the writer cannot compile-check (a second platform is not covered).

### 107. `F-PLAT-MODELS-B-B` Platform models II: Appearance settings: colour scheme page, theme preview, icon themes, contrast, background, ligatures, e2e

- Webview sources (lines): see the spec files; spec files 1261 lines.
- Native neighbours: `app-gpui/src/theme/fonts.rs`, `app-gpui/src/theme/color_scheme.rs`, `app-gpui/src/theme/icls.rs`, `app-gpui/src/theme/themes.rs`, `app-gpui/src/platform/css_color.rs`, `app-gpui/src/settings_view/pages` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:editor.colorScheme.delete`, `act:editor.colorScheme.export`, `act:editor.colorScheme.import`, `act:view.quickSwitchScheme`, `set:(at) src/features/settings/iconThemePages.tsx:18`, `set:appearance`, `ipc:settings_removed_theme`, `ipc:system_font_families`, `ipc:user_font_data`, `unit:src/features/settings/__tests__/backgroundImage.test.ts`, `unit:src/features/settings/__tests__/colorScheme.test.tsx`, `unit:src/features/settings/__tests__/contrast.test.ts`, `unit:src/features/settings/__tests__/fontSync.test.ts`, `unit:src/features/settings/__tests__/iconThemeSettings.test.tsx`, `unit:src/features/settings/__tests__/ligatures.test.ts`, `unit:src/features/settings/__tests__/themePreview.test.tsx`, `e2e:e2e/icls-php.spec.ts`, `e2e:e2e/themes.spec.ts`.
- Spec files and cases (total cases at HEAD; 47 still to prove): `e2e/icls-php.spec.ts` 1, `e2e/themes.spec.ts` 5, `src/features/settings/__tests__/backgroundImage.test.ts` 6, `src/features/settings/__tests__/colorScheme.test.tsx` 20, `src/features/settings/__tests__/contrast.test.ts` 10, `src/features/settings/__tests__/fontSync.test.ts` 1, `src/features/settings/__tests__/iconThemeSettings.test.tsx` 5, `src/features/settings/__tests__/ligatures.test.ts` 2, `src/features/settings/__tests__/themePreview.test.tsx` 4.
- Never in tests or the real window: HOME (system font listing reads the machine's fonts: read only) (HOME: nothing of the user's home).
- Size (Hypothesis): read about 1261 lines, write about 1566; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 78 %; Hypothesis).
- Depends on: none.

### 108. `F-SET-PYTHON-PHP-A` Python interpreters (status, add interpreter, container interpreter, docstrings) and PHP language level: Python interpreters

- Webview sources (lines): see the spec files; spec files 1002 lines.
- Native neighbours: `app-gpui/src/settings_view/pages`, `app-gpui/src/settings_model/builtin_pages` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:python.addInterpreter`, `act:python.interpreterPopup`, `act:python.interpreterSettings`, `act:python.selectInterpreter`, `stat:python.interpreter`, `set:python`, `set:python.docstrings`, `set:python.interpreter`, `set:python.interpreters`, `set:python.ruff`, `ipc:sdk_python_interpreter`, `ipc:sdk_python_project_interpreters`, `unit:src/features/settings/__tests__/addInterpreter.test.tsx`, `unit:src/features/settings/__tests__/containerInterpreter.test.tsx`, `unit:src/features/settings/__tests__/interpreters.test.ts`, `unit:src/features/settings/__tests__/pythonInterpreters.test.tsx`, `unit:src/features/settings/__tests__/pythonStatus.test.tsx`, `e2e:e2e/pythonInterpreters.spec.ts`.
- Spec files and cases (total cases at HEAD; 46 still to prove): `e2e/pythonInterpreters.spec.ts` 2, `src/features/settings/__tests__/addInterpreter.test.tsx` 10, `src/features/settings/__tests__/containerInterpreter.test.tsx` 3, `src/features/settings/__tests__/interpreters.test.ts` 8, `src/features/settings/__tests__/pythonInterpreters.test.tsx` 7, `src/features/settings/__tests__/pythonStatus.test.tsx` 16.
- Never in tests or the real window: TOOL (no interpreter is run: the detection seam answers; never Add Interpreter against the machine) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 1002 lines, write about 1878; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 83 %; Hypothesis).
- Depends on: none.

### 109. `F-DB-GRID-A` Database grid: JSON value window and cell editors

- Webview sources (lines): `src/features/database/cellEditors.ts` (134), `src/features/database/jsonValue.ts` (77); spec files 323 lines.
- Native neighbours: `app-gpui/src/database/grid.rs`, `app-gpui/src/database/grid_view.rs`, `app-gpui/src/database/result_editing.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:db.findInValue`, `act:db.openValueWindow`, `act:db.reformatValue`, `ctx:dbJsonValueCanOpenWindow`, `ctx:dbJsonValueEditable`, `ctx:dbJsonValueFocus`, `unit:src/features/database/cellEditors.test.ts`, `unit:src/features/database/jsonValue.test.ts`, `e2e:e2e/databaseJsonValue.spec.ts`.
- Spec files and cases (total cases at HEAD; 21 still to prove): `e2e/databaseJsonValue.spec.ts` 2, `src/features/database/cellEditors.test.ts` 8, `src/features/database/jsonValue.test.ts` 11.
- Never in tests or the real window: TOOL (SQLite in the temp folder only; no server database) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 534 lines, write about 991; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none (slices: I-ACTIONS-ENABLED).

### 110. `F-VCS-CHANGES` Local Changes view: grouping by repository, directories, copy, ignore submenu, drops onto change lists, test-file colour

- Webview sources (lines): `src/features/vcs/CommitView.tsx` (1414), `src/features/vcs/changes.ts` (533), `src/features/vcs/changelistsUi.tsx` (352), `src/features/vcs/changelists.ts` (256); spec files 1840 lines.
- Native neighbours: `app-gpui/src/vcs_ops/commit_view.rs`, `app-gpui/src/vcs/changes.rs`, `app-gpui/src/vcs_ops/changelists_ui.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `tw:commit`, `ipc:vcs_file_mode`, `ipc:vcs_status_paths`, `unit:src/features/vcs/__tests__/changelistDrop.test.ts`, `unit:src/features/vcs/__tests__/changes.test.ts`, `unit:src/features/vcs/__tests__/changesCopy.test.tsx`, `unit:src/features/vcs/__tests__/changesDirectory.test.tsx`, `unit:src/features/vcs/__tests__/changesGroupByRepo.test.tsx`, `unit:src/features/vcs/__tests__/changesIgnoreMenu.test.tsx`, `unit:src/features/vcs/__tests__/changesRepoBranch.test.tsx`, `unit:src/features/vcs/__tests__/deleteFiles.test.ts`, `unit:src/features/vcs/__tests__/localChanges.test.tsx`, `unit:src/features/vcs/__tests__/multiRepoRollback.test.ts`, `unit:src/features/vcs/__tests__/rollbackAction.test.ts`, `unit:src/features/vcs/__tests__/testFileColor.test.tsx`, `e2e:e2e/commitDoubleClick.spec.ts`, `e2e:e2e/commitDrag.spec.ts`.
- Spec files and cases (total cases at HEAD; 64 still to prove): `e2e/commitDoubleClick.spec.ts` 4, `e2e/commitDrag.spec.ts` 1, `src/features/vcs/__tests__/changelistDrop.test.ts` 4, `src/features/vcs/__tests__/changes.test.ts` 31, `src/features/vcs/__tests__/changesCopy.test.tsx` 4, `src/features/vcs/__tests__/changesDirectory.test.tsx` 1, `src/features/vcs/__tests__/changesGroupByRepo.test.tsx` 5, `src/features/vcs/__tests__/changesIgnoreMenu.test.tsx` 3, `src/features/vcs/__tests__/changesRepoBranch.test.tsx` 2, `src/features/vcs/__tests__/deleteFiles.test.ts` 3, `src/features/vcs/__tests__/localChanges.test.tsx` 21, `src/features/vcs/__tests__/multiRepoRollback.test.ts` 4, `src/features/vcs/__tests__/rollbackAction.test.ts` 6, `src/features/vcs/__tests__/testFileColor.test.tsx` 2.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 4395 lines, write about 2196; split into 2: tree first, then drops and copy.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 76 %; Hypothesis).
- Depends on: Q-FU4.

### 111. `F-RUN-CONSOLE` Run console: colouring by level (console.logColors), copy, selection, find bar, output sample, terminal console

- Webview sources (lines): `src/features/run/ConsoleView.tsx` (630), `src/features/run/console.ts` (182), `src/ui/consoleFind.ts` (81), `src/ui/ConsoleFindBar.tsx` (147), `src/ui/logColors.ts` (53), `src/features/run/useConsoleSelection.ts` (73); spec files 1293 lines.
- Native neighbours: `app-gpui/src/run/console.rs`, `app-gpui/src/run/ansi.rs`, `app-gpui/src/ui/console_find.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:run.sampleOutput`, `set:build.console`, `mod:src/ui/consoleFind.ts`, `mod:src/ui/ConsoleFindBar.tsx`, `mod:src/ui/logColors.ts`, `unit:src/features/app/__tests__/consoleCopy.test.ts`, `unit:src/features/run/consoleColoring.test.tsx`, `unit:src/features/run/consoleCopy.test.tsx`, `unit:src/features/run/findBar.test.tsx`, `unit:src/features/run/runOutput.test.ts`, `unit:src/features/run/terminal.test.tsx`, `unit:src/features/run/useConsoleSelection.test.tsx`, `unit:src/ui/consoleFind.test.ts`, `unit:src/ui/logColors.test.ts`, `e2e:e2e/console-selection.spec.ts`, `e2e:e2e/console-ui.spec.ts`, `e2e:e2e/consoleFont.spec.ts`.
- Spec files and cases (total cases at HEAD; 46 still to prove): `e2e/console-selection.spec.ts` 4, `e2e/console-ui.spec.ts` 1, `e2e/consoleFont.spec.ts` 1, `src/features/app/__tests__/consoleCopy.test.ts` 12, `src/features/run/consoleColoring.test.tsx` 5, `src/features/run/consoleCopy.test.tsx` 4, `src/features/run/findBar.test.tsx` 9, `src/features/run/runOutput.test.ts` 2, `src/features/run/terminal.test.tsx` 7, `src/features/run/useConsoleSelection.test.tsx` 5, `src/ui/consoleFind.test.ts` 11, `src/ui/logColors.test.ts` 4.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 2459 lines, write about 1536; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 71 %; Hypothesis).
- Depends on: none.

### 112. `F-DBG-HEAP-MEMORY` Heap snapshots and JVM memory view: capture, open, save, compare, dominators, instances, referrers

- Webview sources (lines): `src/features/debugger/heap` (1257), `src/features/debugger/memory` (1285); spec files 2920 lines.
- Native neighbours: `app-gpui/src/debugger/state.rs`, `app-gpui/src/debugger/window_view.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:debug.heap.capture`, `act:debug.heap.open`, `act:debug.heap.saveDump`, `act:debug.jvmAttachReady`, `act:debug.memory.closeInstances`, `act:debug.memory.jumpToSource`, `act:debug.memory.refresh`, `act:debug.memory.showInstances`, `act:debug.memory.showReferrers`, `ctx:debugHasJvmMemory`, `ctx:debugHeapSnapshotCaptured`, `kind:heap-snapshot`, `ipc:debug_heap_capture`, `ipc:debug_heap_classes`, `ipc:debug_heap_close`, `ipc:debug_heap_compare`, `ipc:debug_heap_dominators`, `ipc:debug_heap_open`, `ipc:debug_heap_path`, `ipc:debug_heap_save`, `ipc:debug_jvm_memory_classes`, `ipc:debug_jvm_memory_fields`, `ipc:debug_jvm_memory_instances`, `ipc:debug_jvm_memory_referrers`, `unit:src/features/debugger/__tests__/heapEdgeText.test.ts`, `unit:src/features/debugger/__tests__/heapEditor.test.tsx`, `unit:src/features/debugger/__tests__/heapState.test.ts`, `unit:src/features/debugger/__tests__/instancesPane.test.tsx`, `unit:src/features/debugger/__tests__/instancesState.test.ts`, `unit:src/features/debugger/__tests__/memoryLayout.test.ts`, `unit:src/features/debugger/__tests__/memoryRows.test.ts`, `unit:src/features/debugger/__tests__/memoryState.test.ts`, `unit:src/features/debugger/__tests__/memoryValues.test.ts`, `unit:src/features/debugger/__tests__/memoryView.test.tsx`.
- Spec files and cases (total cases at HEAD; 183 still to prove): `src/features/debugger/__tests__/heapEdgeText.test.ts` 4, `src/features/debugger/__tests__/heapEditor.test.tsx` 21, `src/features/debugger/__tests__/heapState.test.ts` 41, `src/features/debugger/__tests__/instancesPane.test.tsx` 22, `src/features/debugger/__tests__/instancesState.test.ts` 19, `src/features/debugger/__tests__/memoryLayout.test.ts` 5, `src/features/debugger/__tests__/memoryRows.test.ts` 11, `src/features/debugger/__tests__/memoryState.test.ts` 9, `src/features/debugger/__tests__/memoryValues.test.ts` 7, `src/features/debugger/__tests__/memoryView.test.tsx` 44.
- Never in tests or the real window: TOOL (heap files are small fixtures written by the test; no JVM is attached), DEBUG (TOOL: no machine toolchain or real external program; DEBUG: stand-in debug adapter only).
- Size (Hypothesis): read about 5462 lines, write about 5461; split into 4: heap snapshot (state + editor, 66 cases), then memory view (88 cases), then instances (41).
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.

### 113. `F-DBG-TAIL` Debugger: actions spec, launch, Java, hotswap, tool window, the feature row

- Webview sources (lines): `src/features/debugger/actions.ts` (1073), `src/features/debugger/launch.ts` (481), `src/features/debugger/java.ts` (273), `src/features/debugger/hotswap.ts` (254), `src/features/debugger/startFailure.ts` (21); spec files 2030 lines.
- Native neighbours: `app-gpui/src/debugger` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:debug.pluginStart`, `pt:debugger`, `pt:debugger.hotswap`, `pt:debugger.stop`, `ovl:debugger`, `tw:debug`, `feat:debugger`, `ipc:plugin_prepare_debug_run`, `unit:src/features/debugger/__tests__/actions.test.ts`, `unit:src/features/debugger/__tests__/java.test.ts`, `unit:src/features/debugger/__tests__/launch.test.ts`, `unit:src/features/debugger/__tests__/startFailure.test.ts`, `unit:src/features/debugger/__tests__/toolWindow.test.tsx`.
- Spec files and cases (total cases at HEAD; 34 still to prove): `src/features/debugger/__tests__/actions.test.ts` 27, `src/features/debugger/__tests__/java.test.ts` 14, `src/features/debugger/__tests__/launch.test.ts` 27, `src/features/debugger/__tests__/startFailure.test.ts` 5, `src/features/debugger/__tests__/toolWindow.test.tsx` 22.
- Never in tests or the real window: DEBUG (DEBUG: stand-in debug adapter only).
- Size (Hypothesis): read about 4132 lines, write about 1647; split into 2 by the native neighbours it touches.
- Depends on: the folder's other slices (slices: F-DBG-ATTACH, F-DBG-CONSOLE-WATCHES-A, F-DBG-CONSOLE-WATCHES-B, F-DBG-HEAP-MEMORY, F-DBG-JS-BROWSER).

### 114. `F-DB-GRID-B` Database grid: Grid spec: booleans, enums, NULL / DEFAULT, copy, filter, rows

- Webview sources (lines): `src/features/database/Grid.tsx` (885); spec files 982 lines.
- Native neighbours: `app-gpui/src/database/grid.rs`, `app-gpui/src/database/grid_view.rs`, `app-gpui/src/database/result_editing.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:db.addRow`, `act:db.copyAsCsv`, `act:db.copyAsOneRow`, `act:db.copyAsTsv`, `act:db.copyRowsAsCsv`, `act:db.copyRowsAsTsv`, `act:db.copyWithHeader`, `act:db.deleteRows`, `act:db.editCell`, `act:db.localFilter`, `act:db.restoreRows`, `act:db.setCellDefault`, `act:db.setCellNull`, `act:db.toggleBooleanCell`, `ctx:dbCellGridFocus`, `unit:src/features/database/Grid.test.tsx`, `e2e:e2e/databaseNumericSort.spec.ts`.
- Spec files and cases (total cases at HEAD; 30 still to prove): `e2e/databaseNumericSort.spec.ts` 3, `src/features/database/Grid.test.tsx` 39.
- Never in tests or the real window: TOOL (SQLite in the temp folder only; no server database) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 1867 lines, write about 2589; split into 2: JSON value window, then booleans/enums/NULL/DEFAULT, then copy/filter.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 94 %; Hypothesis).
- Depends on: none (slices: I-ACTIONS-ENABLED).

### 115. `I-STAGE-HUNKS` Stage hunks from the diff toolbar (integrator-direct, per the backlog)

- Webview sources (lines): `src/features/vcs/diffControls.ts` (154); spec files 0 lines.
- Native neighbours: `app-gpui/src/vcs_diff/toolbar_view.rs`, `app-gpui/src/vcs_diff/diff_controls.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `ipc:vcs_stage_hunks`.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 154 lines, write about 200; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.

### 116. `F-NAVIGATION` Navigation: related symbol, line markers (33 cases), Python hierarchy, header source, implementation gutter, inheritance graph, goto test, super method, structure

- Webview sources (lines): `src/editor/navigation` (2383), `src/editor/navigationProviders.ts` (86), `src/editor/relatedProviders.ts` (47), `src/editor/navigation/gotoTest.ts` (130), `src/editor/navigation/headerSource.ts` (53), `src/editor/navigation/implementationGutter.ts` (820), `src/editor/navigation/index.ts` (264), `src/editor/navigation/inheritanceGraph.ts` (339), `src/editor/navigation/lineMarkers.ts` (318), `src/editor/navigation/pythonHierarchy.ts` (179), `src/editor/navigation/structureNav.ts` (166), `src/editor/navigation/testNaming.ts` (114), `src/editor/structure/index.ts` (187); spec files 1310 lines.
- Native neighbours: `app-gpui/src/editor_refactor/navigation_providers.rs`, `app-gpui/src/editor_refactor/goto_test.rs`, `app-gpui/src/editor/structure` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:navigate.gotoTest`, `act:navigate.relatedSymbol`, `act:navigate.superMethod`, `mod:src/editor/navigation/gotoTest.ts`, `mod:src/editor/navigation/headerSource.ts`, `mod:src/editor/navigation/implementationGutter.ts`, `mod:src/editor/navigation/index.ts`, `mod:src/editor/navigation/inheritanceGraph.ts`, `mod:src/editor/navigation/lineMarkers.ts`, `mod:src/editor/navigation/pythonHierarchy.ts`, `mod:src/editor/navigation/structureNav.ts`, `mod:src/editor/navigation/testNaming.ts`, `mod:src/editor/navigationProviders.ts`, `mod:src/editor/relatedProviders.ts`, `mod:src/editor/structure/index.ts`, `ipc:lsp_implementation_locations`, `ipc:lsp_implementations`, `ipc:nav_inheritance`, `ipc:nav_resolve_module`, `ipc:syntax_expression_at`, `unit:src/editor/navigation/headerSource.test.ts`, `unit:src/editor/navigation/lineMarkers.test.ts`, `unit:src/editor/navigation/pythonHierarchy.test.ts`, `unit:src/editor/navigationProviders.test.ts`, `unit:src/editor/structure/structure.test.ts`.
- Spec files and cases (total cases at HEAD; 49 still to prove): `src/editor/navigation/headerSource.test.ts` 3, `src/editor/navigation/lineMarkers.test.ts` 33, `src/editor/navigation/pythonHierarchy.test.ts` 10, `src/editor/navigationProviders.test.ts` 4, `src/editor/structure/structure.test.ts` 6.
- Never in tests or the real window: TOOL (stand-in language server) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 6396 lines, write about 3087; split into 3 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 76 %; Hypothesis).
- Depends on: none.

### 117. `F-VCS-DIFF-TOOLBAR` Diff viewer: toolbar, options, scrolling in step, difference navigation centred

- Webview sources (lines): `src/features/vcs/diffControls.ts` (154), `src/features/vcs/diffOptions.ts` (238), `src/features/vcs/diffScroll.ts` (503), `src/features/vcs/diffNavigation.ts` (25), `src/features/vcs/DiffEditor.tsx` (1607); spec files 1807 lines.
- Native neighbours: `app-gpui/src/vcs_diff/toolbar_view.rs`, `app-gpui/src/vcs_diff/diff_options.rs`, `app-gpui/src/vcs_diff/diff_scroll.rs`, `app-gpui/src/vcs_diff/diff_navigation.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:diff.applyLeftSide`, `act:diff.applyRightSide`, `act:diff.focusOppositePane`, `act:diff.swapSides`, `unit:src/features/vcs/__tests__/diffDividerDrag.test.ts`, `unit:src/features/vcs/__tests__/diffOptions.test.ts`, `unit:src/features/vcs/__tests__/diffScroll.test.ts`, `unit:src/features/vcs/__tests__/diffToolbar.test.tsx`, `unit:src/features/vcs/__tests__/viewerCaret.test.ts`, `e2e:e2e/diff-navigation.spec.ts`.
- Spec files and cases (total cases at HEAD; 24 still to prove): `e2e/diff-navigation.spec.ts` 4, `src/features/vcs/__tests__/diffDividerDrag.test.ts` 4, `src/features/vcs/__tests__/diffOptions.test.ts` 26, `src/features/vcs/__tests__/diffScroll.test.ts` 13, `src/features/vcs/__tests__/diffToolbar.test.tsx` 36, `src/features/vcs/__tests__/viewerCaret.test.ts` 1.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 4334 lines, write about 1616; split into 2 by the native neighbours it touches.
- Depends on: I-STAGE-HUNKS (slices: I-ACTIONS-ENABLED, I-STAGE-HUNKS).

### 118. `F-VCS-DIR-COMPARE` Directory and folder comparison window

- Webview sources (lines): `src/features/vcs/DirectoryCompare.tsx` (292), `src/features/vcs/ComparisonWindow.tsx` (196), `src/features/vcs/ComparisonTree.tsx` (82), `src/features/vcs/comparePaths.ts` (53), `src/features/vcs/comparison` (2160), `src/features/vcs/directoryCompare.tsx` (292); spec files 359 lines.
- Native neighbours: `app-gpui/src/vcs_diff/comparison`, `app-gpui/src/vcs_ops/compare_paths.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:diff.compareDirectories`, `tw:compare`, `ipc:fs_compare_directories`, `ipc:fs_copy_merge`, `ipc:vcs_compare_paths`, `ipc:window_open_comparison`, `unit:src/features/vcs/__tests__/directoryCompare.test.tsx`, `unit:src/features/vcs/__tests__/folderCompare.test.tsx`.
- Spec files and cases (total cases at HEAD; 14 still to prove): `src/features/vcs/__tests__/directoryCompare.test.tsx` 6, `src/features/vcs/__tests__/folderCompare.test.tsx` 8.
- Never in tests or the real window: none (temp folders).
- Size (Hypothesis): read about 3434 lines, write about 1160; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.

### 119. `F-VCS-STASH` Stash Selected Files, multi-root stashes, Unstash view drops, stash drop

- Webview sources (lines): `src/features/vcs/changeOps.tsx` (547), `src/features/vcs/actions.tsx` (780); spec files 379 lines.
- Native neighbours: `app-gpui/src/workbench/vcs_operations.rs`, `app-gpui/src/vcs_ops/change_ops.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vcs.stashSelected`, `ipc:vcs_stash_drop`, `ipc:vcs_stash_push`, `ipc:vcs_stashes`, `unit:src/features/vcs/__tests__/multiRootStashes.test.tsx`, `unit:src/features/vcs/__tests__/stashDialogRepo.test.tsx`, `unit:src/features/vcs/__tests__/stashSelectedFiles.test.ts`, `unit:src/features/vcs/__tests__/unstashViewDrop.test.tsx`.
- Spec files and cases (total cases at HEAD; 14 still to prove): `src/features/vcs/__tests__/multiRootStashes.test.tsx` 2, `src/features/vcs/__tests__/stashDialogRepo.test.tsx` 5, `src/features/vcs/__tests__/stashSelectedFiles.test.ts` 2, `src/features/vcs/__tests__/unstashViewDrop.test.tsx` 5.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 1706 lines, write about 956; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 75 %; Hypothesis).
- Depends on: Q-FU4.

### 120. `F-EDITOR-NAVBAR` Navigation bar and breadcrumbs (top, status bar, hidden), nav children

- Webview sources (lines): `src/editor/core/view/navBar.ts` (79); spec files 102 lines.
- Native neighbours: `app-gpui/src/editor/structure`, `app-gpui/src/workbench/status_bar.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:navigate.navigationBar`, `act:view.navigationBarHidden`, `act:view.navigationBarStatusBar`, `act:view.navigationBarTop`, `ovl:editor.navigationBar`, `stat:editor.breadcrumbs`, `unit:src/editor/core/view/navBar.test.ts`, `unit:src/editor/core/view/navChildren.test.tsx`.
- Spec files and cases (total cases at HEAD; 2 still to prove): `src/editor/core/view/navBar.test.ts` 5, `src/editor/core/view/navChildren.test.tsx` 1.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 181 lines, write about 783; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 88 %; Hypothesis).
- Depends on: none.

### 121. `F-DOTNET` .NET tool window: discover, build, test, restore, packages

- Webview sources (lines): `src/features/dotnet` (665), `src/features/dotnet/store.ts` (315); spec files 97 lines.
- Native neighbours: `app-gpui/src/features_b/dotnet.rs`, `app-gpui/src/platform_more/dotnet.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:dotnet.build`, `act:dotnet.cancel`, `act:dotnet.clean`, `act:dotnet.discover`, `act:dotnet.installTrx`, `act:dotnet.packageChange`, `act:dotnet.packages`, `act:dotnet.reload`, `act:dotnet.restore`, `act:dotnet.search`, `act:dotnet.select`, `act:dotnet.show`, `act:dotnet.test`, `tw:dotnet`, `feat:dotnet`, `unit:src/features/dotnet/store.test.ts`.
- Spec files and cases (total cases at HEAD; 2 still to prove): `src/features/dotnet/store.test.ts` 6.
- Never in tests or the real window: TOOL (no dotnet: the process seam) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 1077 lines, write about 1859; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 94 %; Hypothesis).
- Depends on: F-RUN-TYPES-OTHER (slices: F-RUN-TYPES-OTHER-A, F-RUN-TYPES-OTHER-B, F-RUN-TYPES-OTHER-C).

### 122. `F-AG-MCP` Agents' MCP servers: settings page, per-session MCP picker, import, discover, test (stand-in server)

- Webview sources (lines): `src/features/agents/McpServersPage.tsx` (1562), `src/features/agents/McpPicker.tsx` (418); spec files 842 lines.
- Native neighbours: `app-gpui/src/agents/new_task_view.rs`, `app-gpui/src/settings_view/pages` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:agents.mcpServers`, `set:agents.mcpServers`, `ipc:agent_mcp_discover_sources`, `ipc:agent_mcp_import`, `ipc:agent_mcp_import_selected`, `ipc:agent_mcp_list`, `ipc:agent_mcp_remove`, `ipc:agent_mcp_save`, `ipc:agent_mcp_save_json`, `ipc:agent_mcp_set_enabled`, `ipc:agent_mcp_test`, `ipc:agent_set_mcp_overrides`, `unit:src/features/agents/McpPicker.test.tsx`, `unit:src/features/agents/mcpServers.test.tsx`, `e2e:e2e/agentMcp.spec.ts`.
- Spec files and cases (total cases at HEAD; 12 still to prove): `e2e/agentMcp.spec.ts` 1, `src/features/agents/McpPicker.test.tsx` 2, `src/features/agents/mcpServers.test.tsx` 9.
- Never in tests or the real window: AGENT, NET (MCP Test against a stand-in on loopback only), SECRET (env values and headers to the secret store, never printed) (NET: no network; AGENT: never start a coding agent; SECRET: secret values never in Debug, logs or failure text).
- Size (Hypothesis): read about 2822 lines, write about 1749; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: Q-FU3 (MCP secrets on export).

### 123. `F-DB-RESULT-TABS` Database result tabs: one tab per result set, pin, rename, close variants, paging, request lifecycle, in-editor results, averages

- Webview sources (lines): `src/features/database/ResultsView.tsx` (837), `src/features/database/resultView.ts` (464), `src/features/database/resultTabActions.ts` (131), `src/features/database/requests.ts` (35), `src/features/database/resultEditing.ts` (336); spec files 4075 lines.
- Native neighbours: `app-gpui/src/database/results_view.rs`, `app-gpui/src/database/result_view.rs`, `app-gpui/src/database/result_session.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:db.closeAllButPinnedResultTabs`, `act:db.closeAllResultTabs`, `act:db.closeOtherResultTabs`, `act:db.closeResultTab`, `act:db.firstPage`, `act:db.lastPage`, `act:db.nextPage`, `act:db.nextResultTab`, `act:db.pinResultTab`, `act:db.previousPage`, `act:db.previousResultTab`, `act:db.reloadPage`, `act:db.renameResultTab`, `act:db.toggleInEditorResults`, `ctx:dbResultsFocus`, `ctx:dbTableEditorChanges`, `ctx:dbTableEditorFocus`, `pt:dbResults`, `ipc:db_session_context`, `unit:src/features/database/inEditorResults.test.tsx`, `unit:src/features/database/requestLifecycle.test.ts`, `unit:src/features/database/result-tabs.test.tsx`, `unit:src/features/database/resultEditing.test.ts`, `unit:src/features/database/results-view.test.tsx`, `unit:src/features/database/resultView.test.ts`, `unit:src/features/database/resultViewAverages.test.ts`, `e2e:e2e/databaseAverages.spec.ts`, `e2e:e2e/databaseRequestLifecycle.spec.ts`, `e2e:e2e/databaseResultSpacing.spec.ts`, `e2e:e2e/databaseResultTabs.spec.ts`.
- Spec files and cases (total cases at HEAD; 129 still to prove): `e2e/databaseAverages.spec.ts` 2, `e2e/databaseRequestLifecycle.spec.ts` 4, `e2e/databaseResultSpacing.spec.ts` 1, `e2e/databaseResultTabs.spec.ts` 9, `src/features/database/inEditorResults.test.tsx` 3, `src/features/database/requestLifecycle.test.ts` 44, `src/features/database/result-tabs.test.tsx` 24, `src/features/database/resultEditing.test.ts` 27, `src/features/database/resultView.test.ts` 31, `src/features/database/resultViewAverages.test.ts` 9, `src/features/database/results-view.test.tsx` 33.
- Never in tests or the real window: TOOL (SQLite only) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 5878 lines, write about 4843; split into 4: result tabs, then request lifecycle (44 cases).
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 80 %; Hypothesis).
- Depends on: I-TW-CONTENT-TABS, Q-FU4 (slices: I-ACTIONS-ENABLED, I-TW-CONTENT-TABS).

### 124. `F-LSP-SEMANTIC` Semantic highlighting and syntax fallback (64 cases), syntax errors, temp highlight, usage highlight

- Webview sources (lines): `src/editor/lsp/semantic` (1584), `src/editor/lsp/syntaxErrors.ts` (97), `src/editor/lsp/syntaxFallback.ts` (327), `src/editor/lsp/tempHighlight.ts` (59), `src/editor/lsp/theme.ts` (486), `src/editor/lsp/semantic/go/index.ts` (13), `src/editor/lsp/semantic/index.ts` (333), `src/editor/lsp/semantic/kotlin/index.ts` (55), `src/editor/lsp/semantic/php/constructs.ts` (40), `src/editor/lsp/semantic/python/highlight.ts` (787), `src/editor/lsp/semantic/python/members.ts` (209); spec files 2440 lines.
- Native neighbours: `app-gpui/src/editor_lsp/semantic.rs`, `app-gpui/src/editor_lsp/syntax_errors.rs`, `app-gpui/src/editor/intelligence.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:editor.toggleSemanticHighlighting`, `mod:src/editor/lsp/semantic`, `mod:src/editor/lsp/syntaxErrors.ts`, `mod:src/editor/lsp/syntaxFallback.ts`, `mod:src/editor/lsp/tempHighlight.ts`, `mod:src/editor/lsp/theme.ts`, `ipc:lsp_semantic_tokens`, `unit:src/editor/lsp/semantic/go/index.test.ts`, `unit:src/editor/lsp/semantic/index.test.ts`, `unit:src/editor/lsp/semantic/kotlin/index.test.ts`, `unit:src/editor/lsp/semantic/php/constructs.test.ts`, `unit:src/editor/lsp/semantic/python/highlight.test.ts`, `unit:src/editor/lsp/semantic/python/members.test.ts`, `unit:src/editor/lsp/semantic/typing.test.ts`, `unit:src/editor/lsp/syntaxErrors.test.ts`, `unit:src/editor/lsp/syntaxFallback.test.ts`, `unit:src/editor/lsp/usageHighlight.test.ts`, `e2e:e2e/goHighlight.spec.ts`, `e2e:e2e/kotlinHighlight.spec.ts`, `e2e:e2e/pythonHighlight.spec.ts`, `e2e:e2e/semanticStyle.spec.ts`, `e2e:e2e/usage-highlight-contrast.spec.ts`.
- Spec files and cases (total cases at HEAD; 104 still to prove): `e2e/goHighlight.spec.ts` 1, `e2e/kotlinHighlight.spec.ts` 2, `e2e/pythonHighlight.spec.ts` 1, `e2e/semanticStyle.spec.ts` 1, `e2e/usage-highlight-contrast.spec.ts` 1, `src/editor/lsp/semantic/go/index.test.ts` 4, `src/editor/lsp/semantic/index.test.ts` 2, `src/editor/lsp/semantic/kotlin/index.test.ts` 3, `src/editor/lsp/semantic/php/constructs.test.ts` 4, `src/editor/lsp/semantic/python/highlight.test.ts` 21, `src/editor/lsp/semantic/python/members.test.ts` 5, `src/editor/lsp/semantic/typing.test.ts` 2, `src/editor/lsp/syntaxErrors.test.ts` 3, `src/editor/lsp/syntaxFallback.test.ts` 64, `src/editor/lsp/usageHighlight.test.ts` 5.
- Never in tests or the real window: TOOL (stand-in language server) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 6430 lines, write about 3273; split into 3: syntax fallback alone (64 cases), then semantic per language.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 73 %; Hypothesis).
- Depends on: none.

### 125. `F-VCS-COMBINED-DIFF` Combined Changes Diff (all files in one diff view)

- Webview sources (lines): `src/features/vcs/CombinedDiff.tsx` (221), `src/features/vcs/allInOneDiff.ts` (41), `src/features/vcs/combinedDiff.tsx` (221); spec files 441 lines.
- Native neighbours: `app-gpui/src/vcs_diff`, `app-gpui/src/workbench/diff_tab.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vcs.openDiff`, `act:vcs.showDiff`, `act:vcs.showDiffInEditorTab`, `act:vcs.showDiffInNewWindow`, `kind:combined-diff`, `unit:src/features/vcs/__tests__/combinedDiff.test.tsx`, `unit:src/features/vcs/__tests__/commitPreviewDiff.test.tsx`.
- Spec files and cases (total cases at HEAD; 24 still to prove): `src/features/vcs/__tests__/combinedDiff.test.tsx` 8, `src/features/vcs/__tests__/commitPreviewDiff.test.tsx` 16.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 924 lines, write about 1021; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 71 %; Hypothesis).
- Depends on: none.

### 126. `F-EDITOR-RENDERED-DOCS` Rendered doc comments, Markdown images, rendered view, sticky scroll, relative line numbers, caret row, corner inset, selection layer and marks

- Webview sources (lines): `src/editor/core/cm/bracketBlockSelect.ts` (53), `src/editor/core/cm/caretRow.ts` (31), `src/editor/core/cm/cornerInset.ts` (81), `src/editor/core/cm/docRender.ts` (332), `src/editor/core/cm/markdownImages.ts` (227), `src/editor/core/cm/preview.ts` (42), `src/editor/core/cm/relativeLineNumbers.ts` (41), `src/editor/core/cm/renderedDocs.ts` (894), `src/editor/core/cm/selectionLayer.ts` (381), `src/editor/core/cm/selectionMarks.ts` (55), `src/editor/core/cm/stickyScroll.ts` (339); spec files 1697 lines.
- Native neighbours: `app-gpui/src/editor/view`, `app-gpui/src/workbench/editor_gutter`, `app-gpui/src/editor/marks.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:editor.toggleRenderedDocs`, `act:editor.toggleRenderedView`, `pt:(at) src/editor/core/cm/markdownImages.ts:117`, `unit:src/editor/core/cm/bracketBlockSelect.test.ts`, `unit:src/editor/core/cm/caretRow.test.ts`, `unit:src/editor/core/cm/cornerInset.test.ts`, `unit:src/editor/core/cm/docRender.test.ts`, `unit:src/editor/core/cm/markdownImages.test.ts`, `unit:src/editor/core/cm/preview.test.ts`, `unit:src/editor/core/cm/relativeLineNumbers.test.ts`, `unit:src/editor/core/cm/renderedDocs.gutter.test.ts`, `unit:src/editor/core/cm/renderedDocs.incremental.test.ts`, `unit:src/editor/core/cm/renderedDocs.test.ts`, `unit:src/editor/core/cm/selectionLayer.test.ts`, `unit:src/editor/core/cm/selectionMarks.test.ts`, `unit:src/editor/core/cm/stickyScroll.test.ts`, `unit:src/editor/core/cm/tooltipBounds.test.ts`, `e2e:e2e/cornerInset.spec.ts`, `e2e:e2e/overviewLane.spec.ts`, `e2e:e2e/overviewLaneSelection.spec.ts`, `e2e:e2e/pageScroll.spec.ts`.
- Spec files and cases (total cases at HEAD; 136 still to prove): `e2e/cornerInset.spec.ts` 1, `e2e/overviewLane.spec.ts` 2, `e2e/overviewLaneSelection.spec.ts` 1, `e2e/pageScroll.spec.ts` 2, `src/editor/core/cm/bracketBlockSelect.test.ts` 4, `src/editor/core/cm/caretRow.test.ts` 4, `src/editor/core/cm/cornerInset.test.ts` 4, `src/editor/core/cm/docRender.test.ts` 23, `src/editor/core/cm/markdownImages.test.ts` 7, `src/editor/core/cm/preview.test.ts` 2, `src/editor/core/cm/relativeLineNumbers.test.ts` 3, `src/editor/core/cm/renderedDocs.gutter.test.ts` 20, `src/editor/core/cm/renderedDocs.incremental.test.ts` 6, `src/editor/core/cm/renderedDocs.test.ts` 23, `src/editor/core/cm/selectionLayer.test.ts` 28, `src/editor/core/cm/selectionMarks.test.ts` 3, `src/editor/core/cm/stickyScroll.test.ts` 4, `src/editor/core/cm/tooltipBounds.test.ts` 1.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 4173 lines, write about 3614; split into 3: rendered docs (66 cases), then the rest.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 95 %; Hypothesis).
- Depends on: none.

### 127. `F-SET-PYTHON-PHP-B` Python interpreters (status, add interpreter, container interpreter, docstrings) and PHP language level: PHP language level, Xdebug page

- Webview sources (lines): see the spec files; spec files 360 lines.
- Native neighbours: `app-gpui/src/settings_view/pages`, `app-gpui/src/settings_model/builtin_pages` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:php.languageLevelPopup`, `act:php.selectLanguageLevel`, `stat:php.languageLevel`, `set:languages.php`, `ipc:sdk_php_language_level`, `unit:src/features/settings/__tests__/phpStatus.test.tsx`, `unit:src/features/settings/__tests__/phpXdebug.test.tsx`.
- Spec files and cases (total cases at HEAD; 13 still to prove): `src/features/settings/__tests__/phpStatus.test.tsx` 12, `src/features/settings/__tests__/phpXdebug.test.tsx` 4.
- Never in tests or the real window: TOOL (no interpreter is run: the detection seam answers; never Add Interpreter against the machine) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 360 lines, write about 684; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 71 %; Hypothesis).
- Depends on: none.

### 128. `F-BT-SBT-ANALYZER-A` sbt tool window, Maven/Gradle Dependency Analyzer, Add/Unlink Maven projects, generate sources, skip tests, auto-reload, first import, profile reload: Dependency Analyzer (Maven, Gradle) and Maven project linking

- Webview sources (lines): `src/features/build-tools/DependencyAnalyzer.tsx` (649), `src/features/build-tools/analyzer.ts` (239); spec files 1232 lines.
- Native neighbours: `app-gpui/src/build_tools`, `app-gpui/src/workbench/build_tools.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:gradle.analyzeDependencies`, `act:maven.addProjects`, `act:maven.analyzeDependencies`, `act:maven.importExternal`, `act:maven.unlinkProjects`, `kind:maven.dependencyAnalyzer`, `pt:gradle.analyzeDependencies`, `pt:maven.analyzeDependencies`, `ipc:gradle_dependencies`, `ipc:maven_exclude_dependency`, `unit:src/features/build-tools/analyzer.test.ts`, `unit:src/features/build-tools/DependencyAnalyzer.test.tsx`, `unit:src/features/build-tools/linkage.test.tsx`, `unit:src/features/build-tools/workspace.test.tsx`.
- Spec files and cases (total cases at HEAD; 29 still to prove): `src/features/build-tools/DependencyAnalyzer.test.tsx` 6, `src/features/build-tools/analyzer.test.ts` 7, `src/features/build-tools/linkage.test.tsx` 24, `src/features/build-tools/workspace.test.tsx` 5.
- Never in tests or the real window: TOOL (no Maven, Gradle or sbt runs: the backend's switches stay off and answers are recorded), HOME (no ~/.m2, ~/.gradle, mise config) (TOOL: no machine toolchain or real external program; HOME: nothing of the user's home).
- Size (Hypothesis): read about 2120 lines, write about 1792; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 93 %; Hypothesis).
- Depends on: none.

### 129. `F-BT-TAIL` Build tools: the feature, Maven/Gradle tool windows, Execute popup and remaining rows

- Webview sources (lines): `src/features/build-tools/index.ts` (577); spec files 0 lines.
- Native neighbours: `app-gpui/src/build_tools` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:gradle.executeTask`, `act:maven.executeGoal`, `act:maven.settings`, `ovl:build-tools.execute`, `pt:build-tools`, `tw:gradle`, `tw:maven`, `feat:build-tools`.
- Never in tests or the real window: TOOL (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 577 lines, write about 441; one writer.
- Depends on: the folder's other slices (slices: F-BT-POM, F-BT-SBT-ANALYZER-A, F-BT-SBT-ANALYZER-B).

### 130. `F-AG-TAIL` Agents: the feature, tool window, editor kinds and remaining rows

- Webview sources (lines): `src/features/agents/index.ts` (338); spec files 0 lines.
- Native neighbours: `app-gpui/src/agents`, `app-gpui/src/workbench/agents.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:(at) src/features/agents/index.ts:210`, `kind:agent-home`, `kind:agent-session`, `pt:(at) src/features/agents/fontZoom.ts:83`, `pt:(at) src/features/agents/Composer.tsx:871`, `tw:agents`, `feat:agents`.
- Never in tests or the real window: AGENT (AGENT: never start a coding agent).
- Size (Hypothesis): read about 338 lines, write about 568; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 71 %; Hypothesis).
- Depends on: the folder's other slices (slices: F-AG-COMPOSER-TASKS-A, F-AG-COMPOSER-TASKS-B, F-AG-COMPOSER-TASKS-C, F-AG-MCP, F-AG-MODELS, F-AG-SESSION-FEATURES).

### 131. `I-ZOOM-PX` UI zoom reaches spacing and sizes written in pixels (~2,600 places, mechanical `theme::z(px)` sweep per directory)

- Webview sources (lines): `src/platform/zoom.ts` (7), `src/editor/core/cm/prefs.ts` (670); spec files 95 lines.
- Native neighbours: `app-gpui/src/theme/zoom.rs`, `app-gpui/src/workbench/zoom_actions.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:editor.decreaseFontSize`, `act:editor.increaseFontSize`, `act:editor.resetFontSize`, `act:view.resetZoom`, `act:view.zoomIn`, `act:view.zoomOut`, `mod:src/platform/zoom.ts`, `unit:src/editor/core/fontZoom.test.ts`.
- Spec files and cases (total cases at HEAD; 6 still to prove): `src/editor/core/fontZoom.test.ts` 6.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 772 lines, write about 2600; split into 2: by native directory (vcs, database, workbench, …), one integration each, no behaviour change.
- Depends on: none.

### 132. `F-PL-TAIL` Plugins: the feature and the remaining rows

- Webview sources (lines): `src/features/plugins/index.ts` (174), `src/features/plugins/languageVisuals.ts` (208), `src/features/plugins/navigation.ts` (267), `src/features/plugins/runtimeUi.tsx` (762); spec files 512 lines.
- Native neighbours: `app-gpui/src/plugin_models`, `app-gpui/src/plugins.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `feat:plugins`, `unit:src/features/plugins/languageVisuals.test.ts`, `unit:src/features/plugins/navigation.test.ts`, `unit:src/features/plugins/runtimeUi.test.tsx`.
- Spec files and cases (total cases at HEAD; 10 still to prove): `src/features/plugins/languageVisuals.test.ts` 5, `src/features/plugins/navigation.test.ts` 11, `src/features/plugins/runtimeUi.test.tsx` 7.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 1923 lines, write about 673; one writer.
- Depends on: the folder's other slices (slices: F-PL-DETAILS, F-PL-NAV-EVENTS, F-PL-VIEWS-A, F-PL-VIEWS-B, F-PL-VIEWS-C).

### 133. `F-DEPLOY-WINDOW` Deployment: window of its own, Download Remote Into, Remote Host, configure, upload to

- Webview sources (lines): `src/features/deployment` (2429), `src/features/deployment/actions.tsx` (593), `src/features/deployment/model.ts` (201), `src/features/deployment/remoteHost.ts` (211), `src/features/deployment/window.tsx` (187); spec files 704 lines.
- Native neighbours: `app-gpui/src/deployment`, `app-gpui/src/features_b/deployment` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:deployment.configure`, `act:deployment.downloadRemoteInto`, `act:deployment.uploadTo`, `pt:deployment`, `ovl:deployment.dialogs`, `tw:remoteHost`, `feat:deployment`, `ipc:remote_download_into`, `ipc:window_open_deployment`, `unit:src/features/deployment/actions.test.tsx`, `unit:src/features/deployment/model.test.ts`, `unit:src/features/deployment/remoteHost.test.ts`, `unit:src/features/deployment/window.test.ts`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/features/deployment/actions.test.tsx` 13, `src/features/deployment/model.test.ts` 14, `src/features/deployment/remoteHost.test.ts` 4, `src/features/deployment/window.test.ts` 2.
- Never in tests or the real window: NET (local-folder servers only) (NET: no network).
- Size (Hypothesis): read about 4325 lines, write about 1800; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 31 %; Hypothesis).
- Depends on: Q-RC, T1-DEPLOY-FTP (slices: T1-DEPLOY-FTP).

### 134. `F-RUN-TABS` Run and Debug tabs: pin, close tab / others / all, pinned by default (run.pinTabs), rerun keeps its pin

- Webview sources (lines): `src/features/run/runTabs.ts` (95), `src/features/debugger/tabActions.ts` (94), `src/features/debugger/state.ts` (1398); spec files 57 lines.
- Native neighbours: `app-gpui/src/run/store.rs`, `app-gpui/src/run/view.rs`, `app-gpui/src/workbench/debug.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:debug.closeAllTabs`, `act:debug.closeOtherTabs`, `act:debug.closeTab`, `act:debug.pinTab`, `act:debug.rerun`, `act:run.closeAllTabs`, `act:run.closeOtherTabs`, `act:run.closeTab`, `act:run.pinTab`, `boundid:toolwindow.debug`, `set:build.run`, `unit:src/features/debugger/__tests__/closeTab.test.ts`, `unit:src/features/debugger/__tests__/sessionTabs.test.ts`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/features/debugger/__tests__/closeTab.test.ts` 1, `src/features/debugger/__tests__/sessionTabs.test.ts` 3.
- Never in tests or the real window: TOOL (stand-in runner), DEBUG (TOOL: no machine toolchain or real external program; DEBUG: stand-in debug adapter only).
- Size (Hypothesis): read about 1644 lines, write about 1707; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 85 %; Hypothesis).
- Depends on: I-TW-CONTENT-TABS, T1-RUN-GUARDS (slices: I-ACTIONS-ENABLED, I-TW-CONTENT-TABS, T1-RUN-GUARDS).

### 135. `F-TOOLS-SERVERS` Tools: Language Servers page, install prompts (listed, never run), expired tools, JDK tool

- Webview sources (lines): `src/features/tools` (2237), `src/features/tools/expired.ts` (49), `src/features/tools/jdk.ts` (128), `src/features/tools/serverSettings.tsx` (661); spec files 1081 lines.
- Native neighbours: `app-gpui/src/settings_view/pages`, `app-gpui/src/features_b/mod.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:java.downloadJdk`, `act:tools.install`, `act:tools.languageServers`, `act:tools.promptInstall`, `set:languages.servers`, `feat:tools`, `ipc:lsp_restart_named`, `unit:src/features/tools/__tests__/expired.test.ts`, `unit:src/features/tools/__tests__/jdk.test.ts`, `unit:src/features/tools/__tests__/serverSettings.test.tsx`, `unit:src/features/tools/__tests__/tools.test.tsx`.
- Spec files and cases (total cases at HEAD; 51 still to prove): `src/features/tools/__tests__/expired.test.ts` 4, `src/features/tools/__tests__/jdk.test.ts` 5, `src/features/tools/__tests__/serverSettings.test.tsx` 11, `src/features/tools/__tests__/tools.test.tsx` 31.
- Never in tests or the real window: TOOL, NET (installs: stand-ins only; tools_install is in C-TOOLCHAIN) (NET: no network; TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 4156 lines, write about 1983; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.

### 136. `I-SOFTWRAP` Soft wrap in the editor (View › Soft-Wrap, gutter menu row, soft-wrap surfaces)

- Webview sources (lines): `src/workbench/softWrap.ts` (39), `src/editor/core/cm` (14414), `src/editor/core/gutterMenu.ts` (52); spec files 0 lines.
- Native neighbours: `app-gpui/src/editor/view`, `app-gpui/src/vcs_diff/wrap.rs`, `app-gpui/src/workbench/editor_gutter` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:editor.toggleSoftWrap`, `ctx:softWrapSurfaceFocus`, `pt:(at) src/features/vcs/DiffEditor.tsx:453`, `mod:src/editor/core/gutterMenu.ts`, `mod:src/workbench/softWrap.ts`.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 14505 lines, write about 1400; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 80 %; Hypothesis).
- Depends on: none (slices: I-ACTIONS-ENABLED).
- Note: Vim's gj/gk/g0/g$ (vim design R6) and the diff's soft-wrap surface need it.

### 137. `F-VCS-SYNC` Root sync, repository-scoped sync, Sync Commits popup, status merge

- Webview sources (lines): `src/features/vcs/SyncCommitsPopup.tsx` (245), `src/features/vcs/BranchSync.tsx` (61), `src/features/vcs/rootSync.ts` (204), `src/features/vcs/syncCommits.ts` (71), `src/features/vcs/syncCommitsPopup.tsx` (245); spec files 772 lines.
- Native neighbours: `app-gpui/src/vcs_ops/store.rs`, `app-gpui/src/vcs/branches.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vcs.syncCommits`, `ctx:VCS`, `ovl:vcs.syncCommits`, `unit:src/features/vcs/__tests__/projectSwitch.test.ts`, `unit:src/features/vcs/__tests__/repoScopedSync.test.ts`, `unit:src/features/vcs/__tests__/rootSync.test.ts`, `unit:src/features/vcs/__tests__/statusMerge.test.ts`, `unit:src/features/vcs/__tests__/syncCommits.test.ts`, `unit:src/features/vcs/__tests__/syncCommitsPopup.test.tsx`.
- Spec files and cases (total cases at HEAD; 61 still to prove): `src/features/vcs/__tests__/projectSwitch.test.ts` 1, `src/features/vcs/__tests__/repoScopedSync.test.ts` 7, `src/features/vcs/__tests__/rootSync.test.ts` 18, `src/features/vcs/__tests__/statusMerge.test.ts` 11, `src/features/vcs/__tests__/syncCommits.test.ts` 16, `src/features/vcs/__tests__/syncCommitsPopup.test.tsx` 8.
- Never in tests or the real window: GIT, NET (no fetch from a real remote) (NET: no network; GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 1598 lines, write about 1687; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 89 %; Hypothesis).
- Depends on: none.

### 138. `F-APP-NEW-PROJECT` New Project wizard with scaffolds (catalog, options, requirements, create), new project template, rename project

- Webview sources (lines): `src/features/app/NewProject.tsx` (564), `src/features/app/newProjectTemplate.ts` (87); spec files 49 lines.
- Native neighbours: `app-gpui/src/app_models/project_actions.rs`, `app-gpui/src/app_views` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:file.newProject`, `act:file.renameProject`, `ovl:app.newProject`, `ipc:project_rename`, `ipc:scaffold_catalog`, `ipc:scaffold_create`, `ipc:scaffold_options`, `ipc:scaffold_requirements`, `unit:src/features/app/__tests__/newProjectTemplate.test.ts`.
- Spec files and cases (total cases at HEAD; 3 still to prove): `src/features/app/__tests__/newProjectTemplate.test.ts` 3.
- Never in tests or the real window: NET (scaffold catalog: a stand-in; a scaffold that downloads is listed, not run), TOOL (no build tool runs) (NET: no network; TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 700 lines, write about 1600; split into 2 by the native neighbours it touches.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 78 %; Hypothesis).
- Depends on: none.

### 139. `F-VCS-STAGING` Git staging area (Enable staging area setting, staged/unstaged trees)

- Webview sources (lines): `src/features/vcs/CommitView.tsx` (1414), `src/features/vcs/staging.ts` (51); spec files 194 lines.
- Native neighbours: `app-gpui/src/vcs_ops/commit_view.rs`, `app-gpui/src/settings_model/builtin_pages/vcs.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `set:vcs.git`, `unit:src/features/vcs/__tests__/stagingArea.test.tsx`.
- Spec files and cases (total cases at HEAD; 10 still to prove): `src/features/vcs/__tests__/stagingArea.test.tsx` 10.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 1659 lines, write about 722; one writer.
- Depends on: F-VCS-CHANGES (slices: F-VCS-CHANGES).

### 140. `F-VCS-TAIL` Version control: the feature, its tool windows, gutter and remaining rows (closes after the VCS slices)

- Webview sources (lines): `src/features/vcs/index.ts` (354), `src/features/vcs/shelf.tsx` (548); spec files 234 lines.
- Native neighbours: `app-gpui/src/vcs`, `app-gpui/src/vcs_ops` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `ctx:command`, `pt:vcs.changeMarkers`, `pt:vcs.gutter`, `tw:git`, `feat:vcs`, `ipc:cancel_request`, `ipc:vcs_config_get`, `ipc:vcs_diff_with_worktree`, `ipc:vcs_restore_head`, `ipc:window_take_launch`, `unit:src/features/vcs/__tests__/shelf.test.tsx`.
- Spec files and cases (total cases at HEAD; 4 still to prove): `src/features/vcs/__tests__/shelf.test.tsx` 9.
- Never in tests or the real window: GIT (GIT: temp repositories only, no remote, no user git config).
- Size (Hypothesis): read about 1136 lines, write about 717; one writer.
- Depends on: every F-VCS slice (slices: F-VCS-BRANCHES, F-VCS-CHANGES, F-VCS-COMBINED-DIFF, F-VCS-COMMIT-DIALOG, F-VCS-DIFF-FIND, F-VCS-DIFF-TOOLBAR, F-VCS-DIR-COMPARE, F-VCS-HISTORY, F-VCS-LOG, F-VCS-MERGE, F-VCS-REMOTE-LINKS, F-VCS-REPO-DIALOGS-A, F-VCS-REPO-DIALOGS-B, F-VCS-STAGING, F-VCS-STASH, F-VCS-SYNC).

### 141. `F-SET-TAIL` Settings: the feature row and the remaining Settings rows

- Webview sources (lines): `src/features/settings/index.ts` (263); spec files 0 lines.
- Native neighbours: `app-gpui/src/settings_view`, `app-gpui/src/settings_model` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `feat:settings`, `ipc:ssh_trust_host`.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 263 lines, write about 122; one writer.
- Depends on: the folder's other slices (slices: F-SET-FILE-TYPES, F-SET-JDK-LANGS, F-SET-PYTHON-PHP-A, F-SET-PYTHON-PHP-B, F-SET-UPSTREAM-TOGGLES-A, F-SET-UPSTREAM-TOGGLES-B).

### 142. `F-DB-TAIL` Database: the feature, tool window and remaining rows

- Webview sources (lines): `src/features/database/index.ts` (387); spec files 0 lines.
- Native neighbours: `app-gpui/src/database`, `app-gpui/src/workbench/database.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `tw:database`, `feat:database`.
- Never in tests or the real window: TOOL (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 387 lines, write about 165; one writer.
- Depends on: the folder's other slices (slices: F-DB-CONSOLES, F-DB-GRID-A, F-DB-GRID-B, F-DB-RESULT-TABS, F-DB-TREE-SOURCES-A, F-DB-TREE-SOURCES-B).

### 143. `I-FILE-PROMPT` Folder and file prompts with the webview's title and start folder (choosePath)

- Webview sources (lines): `src/platform/choosePath.ts` (19), `src/platform/dialog.ts` (69); spec files 16 lines.
- Native neighbours: `app-gpui/src/project_view/file_dialogs.rs`, `app-gpui/src/platform/misc.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `mod:src/platform/choosePath.ts`, `mod:src/platform/dialog.ts`, `unit:src/platform/dialog.test.ts`.
- Spec files and cases (total cases at HEAD; 1 still to prove): `src/platform/dialog.test.ts` 1.
- Never in tests or the real window: WIN (the OS panel is never driven by a test; a seam records the request) (WIN: the real window only through the inspection script, neutral paths).
- Size (Hypothesis): read about 104 lines, write about 168; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.

### 144. `F-EXTERNAL-TOOLS` External tools as actions

- Webview sources (lines): `src/features/run/externalTools.ts` (189); spec files 0 lines.
- Native neighbours: `app-gpui/src/workbench/run_configs.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:(dynamic) src/features/run/externalTools.ts:164`.
- Never in tests or the real window: TOOL (a stand-in script) (TOOL: no machine toolchain or real external program).
- Size (Hypothesis): read about 189 lines, write about 186; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: none.

### 145. `F-APP-TAIL` App: the feature row and remaining rows

- Webview sources (lines): `src/features/app/index.ts` (71); spec files 0 lines.
- Native neighbours: `app-gpui/src/app_models` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `feat:app`.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 71 lines, write about 24; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: the folder's other slices (slices: F-APP-NEW-PROJECT, F-APP-SHELL-A, F-APP-SHELL-B, F-APP-SHELL-C, F-APP-WORKSPACE).

### 146. `V-S1` Vim engine S1 keys and keymap

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1200; one writer.
- Depends on: vim design addendum (recheck section 4).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 147. `V-S2` Vim engine S2 state and host

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1000; one writer.
- Depends on: V-S1 (slices: V-S1).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 148. `V-S3` Vim engine S3 motions I

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1300; one writer.
- Depends on: V-S2 (slices: V-S2).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 149. `V-S4` Vim engine S4 motions II

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1200; one writer.
- Depends on: V-S3 (slices: V-S3).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 150. `V-S5` Vim engine S5 operators

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1400; one writer.
- Depends on: V-S3 (slices: V-S3).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 151. `V-S6` Vim engine S6 registers and paste

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1000; one writer.
- Depends on: V-S5 (slices: V-S5).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 152. `V-S7` Vim engine S7 text objects (+ af/if from recheck)

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1000; one writer.
- Depends on: V-S5 (slices: V-S5).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 153. `V-S8` Vim engine S8 insert and replace

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1200; one writer.
- Depends on: V-S5, V-S6 (slices: V-S5, V-S6).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 154. `V-S9` Vim engine S9 repeat, macros, marks, jumps

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1300; one writer.
- Depends on: V-S8 (slices: V-S8).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 155. `V-S11` Vim engine S11 search

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1200; one writer.
- Depends on: V-S9, gate G2 (regex dialect: owner's decision) (slices: V-S9).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 156. `V-S12` Vim engine S12 Ex I

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1400; one writer.
- Depends on: V-S9, V-S11 (slices: V-S11, V-S9).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 157. `V-S13` Vim engine S13 Ex II substitute and global

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1100; one writer.
- Depends on: V-S12 (slices: V-S12).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 158. `V-S10` Vim engine S10 visual

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 1400; one writer.
- Depends on: V-S6, V-S7, V-S8 (slices: V-S6, V-S7, V-S8).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 159. `V-S14` Vim engine S14 multi-cursor and numbers

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none (fake host).
- Size (Hypothesis): read about 479 lines, write about 800; one writer.
- Depends on: V-S10 (slices: V-S10).
- Note: As designed in vim_engine_design.md section 5; closes no row by itself (model only until I1-I4).

### 160. `V-I1` Vim I1 editor seam

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim`, `app-gpui/src/editor/view` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none.
- Size (Hypothesis): read about 479 lines, write about 1000; one writer.
- Depends on: V-S1..V-S5, gates G1 and G3, I-SOFTWRAP (Hypothesis: gj/gk/g0/g$ are display-line moves, design R6; the real host needs soft wrap) (slices: I-SOFTWRAP, V-S1, V-S2, V-S3, V-S4, V-S5).
- Note: Closes no row alone.

### 161. `V-I2A` Vim I2a mode reporting

- Webview sources (lines): `src/features/vim/index.ts` (124); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim`, `app-gpui/src/workbench/status_items.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `ctx:vim.mode`, `ctx:vimMode`, `stat:vim.mode`.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 124 lines, write about 500; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 100 %; Hypothesis).
- Depends on: V-I1 (slices: V-I1).

### 162. `V-I3` Vim I3 app wiring (toggle, reload rc, every editor gets an attachment)

- Webview sources (lines): `src/features/vim/index.ts` (124), `src/features/vim/conflicts.ts` (99), `src/features/vim/rc.ts` (232); spec files 361 lines.
- Native neighbours: `app-gpui/src/features_a/vim`, `app-gpui/src/workbench/menu_spec.rs` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `act:vim.reloadRc`, `act:vim.toggle`, `pt:(at) src/features/vim/index.ts:79`, `set:editor.vim`, `unit:src/features/vim/__tests__/conflicts.test.ts`, `unit:src/features/vim/__tests__/rc.test.ts`.
- Spec files and cases (total cases at HEAD; 9 still to prove): `src/features/vim/__tests__/conflicts.test.ts` 8, `src/features/vim/__tests__/rc.test.ts` 17.
- Never in tests or the real window: HOME (the rc file is the temp home's) (HOME: nothing of the user's home).
- Size (Hypothesis): read about 816 lines, write about 800; one writer.
- Depends on: V-I1, V-I2A, V-S1 (slices: V-I1, V-I2A, V-S1).

### 163. `V-I2B` Vim I2b command line, messages, search highlight

- Webview sources (lines): `src/features/vim/engine.ts` (479); spec files 0 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: none (see note).
- Never in tests or the real window: none.
- Size (Hypothesis): read about 479 lines, write about 900; one writer.
- Depends on: V-I1, V-I3, V-S11..V-S13 (slices: V-I1, V-I3, V-S11, V-S12, V-S13).

### 164. `V-I4` Vim I4 IME, clipboard, the remaining webview cases

- Webview sources (lines): `src/features/vim/engine.ts` (479), `src/features/vim/textObjects.ts` (165); spec files 617 lines.
- Native neighbours: `app-gpui/src/features_a/vim` (each checked to exist at HEAD by `slices.py`).
- Rows it closes or moves: `pt:vim`, `feat:vim`, `unit:src/features/vim/__tests__/textObjects.test.ts`, `unit:src/features/vim/__tests__/vim.test.ts`.
- Spec files and cases (total cases at HEAD; 16 still to prove): `src/features/vim/__tests__/textObjects.test.ts` 5, `src/features/vim/__tests__/vim.test.ts` 22.
- Never in tests or the real window: none.
- Size (Hypothesis): read about 1261 lines, write about 900; one writer.
- Needs a new native component (ranked at 0.6 of its rows per integration; a hand list or more than 70 % of its rows with no record: 75 %; Hypothesis).
- Depends on: V-I1, V-I2A, V-I2B, V-I3, V-S14 (slices: V-I1, V-I2A, V-I2B, V-I3, V-S14).
- Note: The design says 17 of 17 `vim.test.ts` cases; the spec has 22 at HEAD (recheck section 4): the addendum must update it.

## 3. Rows that cannot reach parity by a writer and an integrator alone

### `C-TOOLCHAIN` Needs a real toolchain, daemon, device or network service (52 rows)

What a partial proof can honestly cover: a stand-in (fake server, scripted process, refused loopback port) proves the UI's requests, its handling of answers, errors and cancellation; never that the real tool answers so. The stand-in proofs are carried by: Docker in F-SVC-DOCKER-A/B; Android and WSL, remote development (remote_*, remote-dev shell modules) in F-SMALL-FEATURES-C; tool installs in F-TOOLS-SERVERS; JDK downloads in F-SET-JDK-LANGS; Python and PHP SDK calls in F-SET-PYTHON-PHP-A/B; the notebook kernel in F-NOTEBOOK; updates in F-APP-SHELL-B; the SSH tunnel test and the server-database e2e specs in F-DB-TREE-SOURCES-B; gh_account_repos in F-GH-REVIEW-B; proxy check in F-PLAT-MODELS-C-B. Membership of this bucket is by key pattern (Hypothesis: each row needs the real program; a row whose proof only needs a stand-in moves to its slice).

Rows: `shell:src-tauri/src/remote_container.rs`, `shell:src-tauri/src/remote_dev.rs`, `shell:src-tauri/src/remote_wsl.rs`, `ipc:android_devices`, `ipc:android_logcat`, `ipc:app_check_update`, `ipc:app_install_update`, `ipc:app_update_announcement`, `ipc:db_test_tunnel`, `ipc:docker_compose_op`, `ipc:docker_container_op`, `ipc:docker_container_stats`, `ipc:docker_container_top`, `ipc:docker_events`, `ipc:docker_image_history`, `ipc:docker_inspect`, `ipc:docker_logs`, `ipc:docker_prune`, `ipc:docker_pull`, `ipc:docker_registry_login`, `ipc:docker_remove`, `ipc:docker_run_env`, `ipc:docker_save_settings`, `ipc:docker_set_registry_password`, `ipc:docker_snapshot`, `ipc:gh_account_repos`, `ipc:jdk_download`, `ipc:jdk_download_location`, `ipc:jdk_download_releases`, `ipc:jdk_download_vendors`, `ipc:notebook_execute`, `ipc:notebook_interrupt`, `ipc:notebook_restart`, `ipc:proxy_check_connection`, `ipc:remote_bootstrap_list`, `ipc:remote_connect`, `ipc:remote_forward_port`, `ipc:sdk_container_python`, `ipc:sdk_container_python_refresh`, `ipc:sdk_php_composer_install`, `ipc:sdk_python_create_venv`, `ipc:sdk_python_packages`, `ipc:sdk_python_pip`, `ipc:tools_install`, `ipc:tools_uninstall`, `ipc:tools_updates`, `ipc:wsl_distros`, `ipc:wsl_resolve`, `e2e:e2e/clickhouse-source.spec.ts`, `e2e:e2e/oracle-source.spec.ts`, `e2e:e2e/snowflake-source.spec.ts`, `e2e:e2e/updates.spec.ts`.

### `C-WEBENGINE` Webview-engine and Tauri-shell rows with no native counterpart (manager's decision: no honest end state today, `[–]` is for IPC rows only) (22 rows)

What a partial proof can honestly cover: (membership by key pattern: Hypothesis) none: these test WebKit/Tauri behaviour (clipboard quirks of WebKit, CSP, GTK dialogs, page swipe, webview zoom); a native test can only show the native app does the user-visible thing another way.

Rows: `mod:src/platform/ipc.ts`, `mod:src/platform/reordered-tap-click.ts`, `mod:src/platform/tauriCore.ts`, `mod:src/platform/webkitClipboard.ts`, `shell:src-tauri/src/dev.rs`, `shell:src-tauri/src/jump_list.rs`, `shell:src-tauri/src/linux_webkit.rs`, `shell:src-tauri/src/linux_window_state.rs`, `shell:src-tauri/src/macos_webview.rs`, `shell:src-tauri/src/theme_frame.rs`, `shell:src-tauri/src/windows_error_mode.rs`, `ipc:file_dialog_needs_gtk`, `ipc:gtk_pick_paths`, `ipc:log_frontend`, `ipc:window_set_titlebar_height`, `unit:src/platform/appCsp.test.ts`, `unit:src/platform/ipc.test.ts`, `unit:src/platform/reordered-tap-click.test.ts`, `unit:src/platform/webkitClipboard.test.ts`, `unit:src/platform/webkitCompatibility.test.ts`, `unit:src/styles/cssModules.test.ts`, `unit:src/styles/customProperties.test.ts`.

Also only partly reachable, inside slices of part 2 (their proofs must say what stays unproven):

- Numeric-keypad operator bindings: GPUI reports no physical key (D16; `unit:src/platform/keymap.test.ts`, in `F-PLAT-MODELS-A`).
- System date and time formats other than en-US (`unit:src/platform/dateFormat.test.ts`, `module:src/platform/dateFormat.ts`, in `F-PLAT-MODELS-C`): what WebKit prints per locale was never measured.
- Language-server behaviour (every `F-LSP-*` slice): proven against the stand-in server; the real servers only by the `language_smoke` suite of the backend, which needs the machine's installed servers.
- Debugging (every `F-DBG-*` slice, `F-RUN-TABS`, `T1-RUN-GUARDS`): the stand-in adapter needs the machine's `python3` (OWED line 92); real adapters (CodeLLDB, Delve, the JVM) are not run.
- Agents (every `F-AG-*` slice): a fake agent declared in the test; a real agent is never started (`spawn_guard`).
- Second platform: Windows and Linux behaviour (UNC shares, GTK dialogs, WSL, the Windows PTY `windowsPty.test.ts`, Linux window state) is proven only by text-level tests on macOS; nothing runs on the other platforms.
- System notifications (`I-SYSNOTIFY`): Hypothesis: GPUI has no notification API, so the OS call is platform code proven through a recording seam only.

## Not planned yet (named, with the reason)

Rows: 0 (every open row matched a slice, a queued brief or part 3). Work owed with no inventory row, not planned here:

- Accessibility: the native window exposes almost no accessible elements (OWED line 31, `notes/reports/a11y_assessment.md`: six slices, none started). Reason: no row measures it; the manager decides whether it is parity work.
- Performance owed before the measurement (OWED lines 7, 142, 146): change bars rebuilding an index per edit, a keystroke on a 5 MB file, TODO rescans, VCS branch listing per refresh. Reason: no row; needs a release build and measurements, which this plan cannot size.
- Fonts: the exact upstream Inter 4.001 / JetBrains Mono 2.211 files need a download (OWED line 33). Reason: the owner's call, asked and unanswered.
- `OPENIDEA_DATA_DIR` and the sibling folder variables (RETRIAGE_IPC, follow upstream 3): not followed by the manager's decision.
- The 24 native literals that are no longer keys of the text catalog (RETRIAGE_REST, Locales): they belong to the rows that own the strings; each slice touching one of those files fixes its literal (list in `RETRIAGE_REST.md`).
- Model-only rows: 65 rows are `[m]`; each is in the slice of its feature (their code and tests exist; only the wiring is missing), which makes them the cheapest rows of those slices.

## Notes that contradict another note or the code

Measured unless marked Hypothesis.

- `OWED.md` line 15 says `FormRow`, `FormSpec` and `FormInput` in `app-gpui/src/vcs/git_forms.rs` derive `Debug` and can carry a typed remote URL with credentials, to fix. At HEAD all three have a hand-written `Debug` that prints `<redacted>` (`git_forms.rs:103`, `:187`, `:219`): the entry is stale.
- `vim_engine_design.md` (I4 row, line 436) counts `vim.test.ts` as 17 of 17 cases; the inventory row has 22 cases at HEAD (`rows3.tsv`), as `recheck_after_merge.md` section 4 says. Its section 1 also says the IDE does not use `Vim.defineMotion`, which the merged `textObjects.ts:117` does (recheck section 4). The design is included as is; slice V-S1 depends on the addendum the recheck lists.
- `recheck_after_merge.md` section 6 point 5 contradicts the merge report (`notes/reports/merge_adaptations.md:20`): the report says a plain-text FTP failure is right, the webview at HEAD shows the password remedy (`deployment.tsx:503-508`).
- The merge report says native ignores `AgentEvent::Attention`; `app-gpui/src/agents/store.rs:789` handles it at HEAD (also corrected in `RETRIAGE_IPC.md`).
- `int_remote_chooser_brief.md` line 17 gives the remote chooser the section-6 Deployment mismatches "where they are on the path of the chooser", while `backlog.md` (lines 929, 942, 950, 956) queues "the 16 mismatches of recheck sections 5 and 6" as a later integration of their own. This plan takes sections 5 and 6 as `T1-TEMPLATES-UP` and `T1-DEPLOY-FTP` (after the chooser), minus M6/M7 which the New from Template brief carries as C5/C4.
- `RETRIAGE_ACTIONS.md` says the dispatcher runs the last-bound enabled action on a shared stroke; `int_follow_upstream4_brief.md` item 1 says it takes the first enabled. Both describe the same code (`dispatcher.rs:198-201` iterates the bindings in reverse and takes the first enabled), not a contradiction; the brief's wording is the looser one.
- Five IPC rows have no webview caller: `gh_pr_comment`, `gh_pr_list`, `gh_pr_review`, `gh_pr_review_comments`, `gh_pr_view`: `git grep -n -E 'ghPr(Comment|List|Review|ReviewComments|View)\b|gh_pr_(comment|list|review|view)' -- src` finds them only in `src/bindings.ts`. Hypothesis: they are `[–]` (nothing to port) and the generator's check would accept that; they are in `F-GH-REVIEW` until the manager marks them.
- `NEW_ROWS.md`: `settings:tools.agents` (gone) and `settings:agents.list` (new) are probably one row (Hypothesis there, not checked here); `agents.list` is queued with the Coding Agents brief.
- `OWED.md` line 21 (Restricted Mode: saving run configurations is not trust-checked natively): Hypothesis from reading at HEAD: `ide_host::ops::run::save_configs` (`crates/ide-host/src/ops/run.rs:124`) has no trust check for either app, and both lists are empty while untrusted (`src/features/run/store.ts:225`, `app-gpui/src/run/driver.rs:268`), so the native app may match the webview there; `T1-SAFETY-CHECKS` settles it with a test.
- `callers.py` found no webview listener by name for 17 backend events, among them `FsChanged`, `Notify` and `LanguageServer`, which do have listeners: an artefact of matching the variant name in TypeScript (the payloads are tagged differently), not a finding. Events were assigned to slices by name.

## 4. Totals

- Buckets: 177; slices of this plan: 164 (6 T1, 10 T2 infrastructure, 129 T3 features, 19 Vim as designed); integrations they take with the 1,500-line split: 260.
- Rows covered by slices of this plan: 2423; by queued briefs: 83; cannot reach parity alone: 74; not planned yet: 0; total 2580 = the 2580 open rows.
- Coarse buckets (a folder's remaining rows, planned as the folder's last slice and to be re-planned when it is reached): 10 buckets, 170 rows (F-UI-PRIMITIVES 76, F-VCS-TAIL 11, F-SET-TAIL 2, F-DBG-TAIL 13, F-AG-TAIL 7, F-DB-TAIL 2, F-PL-TAIL 4, F-WIT 46, F-APP-TAIL 1, F-BT-TAIL 8).
- Slices still above 1,500 lines (not split here; each says where to cut): 75, ranked at #131, #74, #117, #58, #110, #137, #67, #32, #38, #36, #81, #107, #16, #75, #56, #124, #37, #62, #39, #87, #96, #126, #71, #44, #83, #45, #134, #40, #111, #97, #30, #77, #108, #89, #112, #105, #122, #54, #84, #46, #114, #123, #47, #48, #94, #41, #100, #31, #59, #65, #49, #138, #95, #21, #93, #98, #57, #128, #25, #53, #43, #34, #116, #91, #85, #121, #80, #135, #133, #66, #15, #92, #17, #113, #102: I-ZOOM-PX, I-WINDOW-GROUPS, F-VCS-DIFF-TOOLBAR, F-VCS-DIFF-FIND, F-VCS-CHANGES, F-VCS-SYNC, F-VCS-COMMIT-DIALOG, F-VCS-LOG, F-VCS-HISTORY, F-PLAT-MODELS-A, F-PLAT-MODELS-B-A, F-PLAT-MODELS-B-B, F-PLAT-MODELS-C-B, F-PLAT-PROJECT-B, F-LSP-NAV, F-LSP-SEMANTIC, F-LSP-INLINE-B, F-LSP-POPUPS-B, F-LSP-POPUPS-C, F-LSP-REFACTOR-A, F-EDITOR-EMBEDDED-LANGS, F-EDITOR-RENDERED-DOCS, F-EDITOR-CORE-EDITING, F-EDITOR-TABS-SESSIONS-B, F-EDITOR-DOCS-SYNC-A, F-EDITOR-DOCS-SYNC-B, F-RUN-TABS, F-RUN-TESTS, F-RUN-CONSOLE, F-RUN-TYPES-JVM, F-RUN-TYPES-OTHER-A, F-RUN-IMPORT, F-SET-PYTHON-PHP-A, F-DBG-JS-BROWSER, F-DBG-HEAP-MEMORY, F-AG-MODELS, F-AG-MCP, F-AG-SESSION-FEATURES, F-AG-COMPOSER-TASKS-B, F-AG-COMPOSER-TASKS-C, F-DB-GRID-B, F-DB-RESULT-TABS, F-DB-CONSOLES, F-DB-TREE-SOURCES-A, F-DB-TREE-SOURCES-B, F-PL-DETAILS, F-PL-VIEWS-A, F-PL-NAV-EVENTS, F-WB-TOOLWINDOWS, F-WB-MENUS-TITLE, F-WB-SHELL-B, F-APP-NEW-PROJECT, F-APP-SHELL-A, F-APP-SHELL-C, F-UI-PRIMITIVES, F-SVC-DOCKER-A, F-SVC-DOCKER-B, F-BT-SBT-ANALYZER-A, F-GH-REVIEW-B, F-PROJECT-VIEW, F-TERMINAL, F-HTTP, F-NAVIGATION, F-ANALYSIS, F-COVERAGE-PROFILER, F-DOTNET, F-MACROS-RUNANY-SCRATCH, F-TOOLS-SERVERS, F-DEPLOY-WINDOW, F-PERF-MCP-SSH, F-SMALL-FEATURES-A, F-SMALL-FEATURES-B, F-SMALL-FEATURES-C, F-DBG-TAIL, F-WIT.
- Spec cases still to prove in planned slices (by the rule above): 4229; lines to write (Hypothesis): 269,291.
