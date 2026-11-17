# New inventory rows after the merge of v0.1.46

Row keys of the inventory generated at HEAD (`8aab59f85`, after the reconciliation) against the inventory at `3662ca0ea`. Gross new keys: **692**; of those **46** are rows that moved, not new (20 re-keyed in parity.json, 18 same registration moved, 6 probably moved: the only gone and the only new key of this point in this file; the lines around it were edited, 2 file renamed); **646** are new rows. Keys of the old inventory with no row now: 55, of which 46 moved and **9** are gone. Totals: 3145 rows before, 3782 now.

A moved row is paired when parity.json was re-keyed for it, when its file was renamed (`git diff -M --name-status`), or when an `(at)`/`(dynamic)` key's four lines around the registration are the same text at the old and the new line of the same file. Rows renamed in another way (a settings page under a new id that parity.json had no entry for) count here as one gone and one new.

Known overstatements of the new count: `boundid:navigate.relatedSymbol` is now the action row `action:navigate.relatedSymbol` (the id is now statically registered), and `settings:tools.agents` (src/features/agents/index.ts:30) looks like `settings:agents.list` (index.ts:61) after upstream's one Settings › Agents page (9e46d61dc); Hypothesis: they are the same rows; check with `git show 3662ca0ea:src/features/agents/index.ts | sed -n 25,40p` against HEAD's lines 55-90. Six of the new Actions rows are commit checks, not actions (`vcs.analysis`, `vcs.crlf`, `vcs.detachedHead`, `vcs.fileNames`, `vcs.largeFiles`, `vcs.todo`, all src/features/vcs/commitChecks.tsx): `tsmodel.mjs` counts their `id` objects as actions while `gen-data.mjs` excludes them (NOT_ACTIONS), so the inventory's Actions total is 6 too high.

## New rows per section

| Section                                                | New rows |
| ------------------------------------------------------ | -------- |
| Behaviour specification: unit tests (vitest)           | 224      |
| Actions                                                | 138      |
| IPC commands                                           | 117      |
| Behaviour specification: end-to-end specs (Playwright) | 43       |
| Editor, platform, workbench and design-system modules  | 37       |
| Context keys                                           | 15       |
| Settings pages                                         | 12       |
| Extension point `registerEditorContribution`           | 8        |
| Extension point `registerNavigationProvider`           | 5        |
| Extension point `registerRunConfigType`                | 5        |
| Editor kinds                                           | 4        |
| Overlays                                               | 4        |
| Features                                               | 4        |
| Native shell modules (src-tauri)                       | 4        |
| Plugin API interfaces (wit/)                           | 4        |
| Extension point `registerAgentContextSource`           | 3        |
| Extension point `registerMenuGroup`                    | 3        |
| Extension point `registerRunExecutor`                  | 3        |
| Extension point `registerEditorBanner`                 | 2        |
| Extension point `registerFontZoomSurface`              | 2        |
| Extension point `registerKeyClaim`                     | 2        |
| Tool windows                                           | 2        |
| Extension point `registerEditorGroupPanel`             | 1        |
| Extension point `registerExternalFileOpenHandler`      | 1        |
| Extension point `registerPendingEdits`                 | 1        |
| Extension point `registerProjectViewRoot`              | 1        |
| Extension point `registerUriOpener`                    | 1        |

## New rows per feature folder

| Folder                           | New rows |
| -------------------------------- | -------- |
| `src-tauri/src/commands`         | 115      |
| `src/features/vcs`               | 60       |
| `src/features/database`          | 59       |
| `src/features/debugger`          | 51       |
| `e2e`                            | 43       |
| `src/features/agents`            | 39       |
| `src/platform`                   | 30       |
| `src/editor/core`                | 29       |
| `src/features/run`               | 25       |
| `src/features/settings`          | 23       |
| `src/ui`                         | 15       |
| `src/workbench`                  | 12       |
| `src/features/build-tools`       | 11       |
| `src/features/terminal`          | 11       |
| `src/editor/lsp`                 | 10       |
| `src/features/http-client`       | 9        |
| `src/features/profiler`          | 9        |
| `src/editor/refactor`            | 9        |
| `src/features/services`          | 7        |
| `src/features/project-tree`      | 7        |
| `src/features/plugins`           | 7        |
| `src/features/app`               | 6        |
| `src/editor/navigation`          | 6        |
| `src-tauri/src`                  | 6        |
| `src/features/viewers`           | 5        |
| `src/features/whats-new`         | 4        |
| `src/features/cucumber`          | 4        |
| `src/features/rails`             | 4        |
| `wit`                            | 4        |
| `src/features/web`               | 3        |
| `src/features/analysis`          | 3        |
| `src/features/thymeleaf`         | 3        |
| `src/styles`                     | 3        |
| `src/features/build`             | 2        |
| `src/features/code-insight`      | 2        |
| `src/features/notebook`          | 2        |
| `src/features/templates`         | 2        |
| `src/features/deployment`        | 1        |
| `src/editor/relatedProviders.ts` | 1        |
| `src/features/color-preview`     | 1        |
| `src/features/libraries`         | 1        |
| `src/features/tools`             | 1        |
| `src/features/vim`               | 1        |

## The 40 largest new spec files by case count (of 267 new spec files)

| Spec                                                       | Cases |
| ---------------------------------------------------------- | ----- |
| `src/features/database/requestLifecycle.test.ts`           | 44    |
| `src/features/debugger/__tests__/memoryView.test.tsx`      | 44    |
| `src/features/debugger/__tests__/heapState.test.ts`        | 41    |
| `src/features/database/result-tabs.test.tsx`               | 24    |
| `src/editor/core/cm/docRender.test.ts`                     | 23    |
| `src/editor/core/cm/renderedDocs.test.ts`                  | 23    |
| `src/features/analysis/inheritanceInspections.test.ts`     | 23    |
| `src/editor/core/cm/linkedTags.test.ts`                    | 22    |
| `src/features/debugger/__tests__/instancesPane.test.tsx`   | 22    |
| `src/features/debugger/__tests__/heapEditor.test.tsx`      | 21    |
| `src/editor/core/cm/renderedDocs.gutter.test.ts`           | 20    |
| `src/features/debugger/__tests__/instancesState.test.ts`   | 19    |
| `src/features/vcs/__tests__/rootSync.test.ts`              | 18    |
| `src/features/agents/composerSubmission.test.tsx`          | 17    |
| `src/features/whats-new/__tests__/whatsNew.test.tsx`       | 17    |
| `src/editor/lsp/navigationShared.test.ts`                  | 16    |
| `src/features/database/export.test.ts`                     | 16    |
| `src/features/vcs/__tests__/syncCommits.test.ts`           | 16    |
| `src/editor/core/cm/django/django.test.ts`                 | 15    |
| `src/features/agents/taskProject.test.ts`                  | 15    |
| `src/features/debugger/__tests__/NetworkView.test.tsx`     | 15    |
| `src/features/cucumber/steps.test.ts`                      | 14    |
| `src/platform/webkitClipboard.test.ts`                     | 14    |
| `src/features/run/executorStop.test.ts`                    | 12    |
| `e2e/agentCycleEffort.spec.ts`                             | 12    |
| `src/features/agents/modelRefresh.test.tsx`                | 11    |
| `src/features/database/jsonValue.test.ts`                  | 11    |
| `src/features/debugger/__tests__/classImports.test.ts`     | 11    |
| `src/features/debugger/__tests__/eventBreakpoints.test.ts` | 11    |
| `src/features/debugger/__tests__/memoryRows.test.ts`       | 11    |
| `src/features/vcs/__tests__/statusMerge.test.ts`           | 11    |
| `e2e/agentFocusInput.spec.ts`                              | 11    |
| `src/editor/lsp/navigationChooser.test.ts`                 | 10    |
| `src/editor/navigation/pythonHierarchy.test.ts`            | 10    |
| `src/features/debugger/__tests__/network.test.ts`          | 10    |
| `src/features/profiler/executor.test.ts`                   | 10    |
| `src/features/vcs/__tests__/stagingArea.test.tsx`          | 10    |
| `src/features/vcs/__tests__/targetBranchInput.test.tsx`    | 10    |
| `src/features/agents/mcpServers.test.tsx`                  | 9     |
| `src/features/agents/SelectionToolbar.test.tsx`            | 9     |

## Moved rows (paired, not counted as new)

| Old key                                                                              | New key                                                                              | How                                                                                                            |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| `action:(dynamic) src/features/plugins/points.tsx:199`                               | `action:(dynamic) src/features/plugins/points.tsx:200`                               | re-keyed in parity.json                                                                                        |
| `action:(dynamic) src/features/search/index.ts:476`                                  | `action:(dynamic) src/features/search/index.ts:484`                                  | re-keyed in parity.json                                                                                        |
| `action:(dynamic) src/features/search/index.ts:486`                                  | `action:(dynamic) src/features/search/index.ts:494`                                  | re-keyed in parity.json                                                                                        |
| `action:(dynamic) src/features/templates/index.ts:79`                                | `action:(dynamic) src/features/templates/index.ts:81`                                | probably moved: the only gone and the only new key of this point in this file; the lines around it were edited |
| `action:(dynamic) src/workbench/toolwindows/actions.ts:160`                          | `action:(dynamic) src/workbench/toolwindows/actions.ts:186`                          | re-keyed in parity.json                                                                                        |
| `action:(dynamic) src/workbench/toolwindows/actions.ts:247`                          | `action:(dynamic) src/workbench/toolwindows/actions.ts:303`                          | re-keyed in parity.json                                                                                        |
| `module:src/editor/lsp/iconMarkup.ts`                                                | `module:src/ui/iconNode.ts`                                                          | file renamed (src/editor/lsp/iconMarkup.ts → src/ui/iconNode.ts)                                               |
| `module:src/platform/filePresentation.ts`                                            | `module:src/ui/filePresentation.ts`                                                  | re-keyed in parity.json                                                                                        |
| `point.registerCommitCheckProvider:(at) src/features/plugins/uiPoints.tsx:212`       | `point.registerCommitCheckProvider:(at) src/features/plugins/uiPoints.tsx:215`       | same registration moved from line 212 to 215                                                                   |
| `point.registerDocumentIndexSync:(at) src/features/plugins/navigation.ts:154`        | `point.registerDocumentIndexSync:(at) src/features/plugins/navigation.ts:170`        | probably moved: the only gone and the only new key of this point in this file; the lines around it were edited |
| `point.registerEditorDropHandler:(at) src/features/agents/index.ts:104`              | `point.registerEditorDropHandler:(at) src/features/agents/index.ts:210`              | same registration moved from line 104 to 210                                                                   |
| `point.registerExtensionBindings:(at) src/features/plugins/uiPoints.tsx:439`         | `point.registerExtensionBindings:(at) src/features/plugins/uiPoints.tsx:446`         | same registration moved from line 439 to 446                                                                   |
| `point.registerFileDropHandler:(at) src/features/terminal/drop.ts:22`                | `point.registerFileDropHandler:(at) src/features/terminal/drop.ts:23`                | probably moved: the only gone and the only new key of this point in this file; the lines around it were edited |
| `point.registerFileEditorProvider:(at) src/features/plugins/uiPoints.tsx:477`        | `point.registerFileEditorProvider:(at) src/features/plugins/uiPoints.tsx:484`        | same registration moved from line 477 to 484                                                                   |
| `point.registerFontZoomSurface:(at) src/features/plugins/views/WebView.tsx:169`      | `point.registerFontZoomSurface:(at) src/features/plugins/views/WebView.tsx:233`      | same registration moved from line 169 to 233                                                                   |
| `point.registerFontZoomSurface:(at) src/features/terminal/session.ts:325`            | `point.registerFontZoomSurface:(at) src/features/terminal/session.ts:382`            | probably moved: the only gone and the only new key of this point in this file; the lines around it were edited |
| `point.registerKeyClaim:(at) src/editor/core/view/FindBar.tsx:367`                   | `point.registerKeyClaim:(at) src/editor/core/view/FindBar.tsx:411`                   | re-keyed in parity.json                                                                                        |
| `point.registerKeyClaim:(at) src/features/debugger/ui/DebugConsole.tsx:244`          | `point.registerKeyClaim:(at) src/features/debugger/ui/DebugConsole.tsx:253`          | same registration moved from line 244 to 253                                                                   |
| `point.registerKeyClaim:(at) src/features/plugins/views/contentEditor.tsx:518`       | `point.registerKeyClaim:(at) src/features/plugins/views/contentEditor.tsx:521`       | same registration moved from line 518 to 521                                                                   |
| `point.registerKeyClaim:(at) src/features/run/ConsoleView.tsx:346`                   | `point.registerKeyClaim:(at) src/features/run/ConsoleView.tsx:362`                   | re-keyed in parity.json                                                                                        |
| `point.registerKeyClaim:(at) src/features/search/everywhere/SearchEverywhere.tsx:82` | `point.registerKeyClaim:(at) src/features/search/everywhere/SearchEverywhere.tsx:86` | re-keyed in parity.json                                                                                        |
| `point.registerKeyClaim:(at) src/features/search/find/parts.tsx:182`                 | `point.registerKeyClaim:(at) src/features/search/find/parts.tsx:184`                 | same registration moved from line 182 to 184                                                                   |
| `point.registerKeyClaim:(at) src/features/vcs/BranchWidget.tsx:297`                  | `point.registerKeyClaim:(at) src/features/vcs/BranchWidget.tsx:368`                  | probably moved: the only gone and the only new key of this point in this file; the lines around it were edited |
| `point.registerLanguageMode:(at) src/features/http-client/index.ts:122`              | `point.registerLanguageMode:(at) src/features/http-client/index.ts:142`              | same registration moved from line 122 to 142                                                                   |
| `point.registerMenuSlotItem:(at) src/features/plugins/uiPoints.tsx:177`              | `point.registerMenuSlotItem:(at) src/features/plugins/uiPoints.tsx:180`              | re-keyed in parity.json                                                                                        |
| `point.registerMenuSlotItem:(at) src/features/plugins/uiPoints.tsx:182`              | `point.registerMenuSlotItem:(at) src/features/plugins/uiPoints.tsx:185`              | re-keyed in parity.json                                                                                        |
| `point.registerNavigationProvider:(at) src/features/plugins/navigation.ts:172`       | `point.registerNavigationProvider:(at) src/features/plugins/navigation.ts:193`       | probably moved: the only gone and the only new key of this point in this file; the lines around it were edited |
| `point.registerOccurrenceNavigator:(at) src/workbench/contributions.ts:1362`         | `point.registerOccurrenceNavigator:(at) src/workbench/contributions.ts:1628`         | re-keyed in parity.json                                                                                        |
| `point.registerProjectViewRoot:(at) src/features/project-tree/index.ts:131`          | `point.registerProjectViewRoot:(at) src/features/project-tree/index.ts:140`          | same registration moved from line 131 to 140                                                                   |
| `point.registerProjectViewRoot:(at) src/features/project-tree/index.ts:141`          | `point.registerProjectViewRoot:(at) src/features/project-tree/index.ts:162`          | same registration moved from line 141 to 162                                                                   |
| `point.registerSaveParticipant:(at) src/editor/lsp/actions.ts:152`                   | `point.registerSaveParticipant:(at) src/editor/lsp/actions.ts:159`                   | same registration moved from line 152 to 159                                                                   |
| `point.registerServiceProvider:(at) src/features/plugins/uiPoints.tsx:372`           | `point.registerServiceProvider:(at) src/features/plugins/uiPoints.tsx:379`           | same registration moved from line 372 to 379                                                                   |
| `point.registerSoftWrapSurface:(at) src/features/vcs/DiffEditor.tsx:384`             | `point.registerSoftWrapSurface:(at) src/features/vcs/DiffEditor.tsx:453`             | same registration moved from line 384 to 453                                                                   |
| `point.registerStatusItem:(at) src/features/plugins/runtimeUi.tsx:159`               | `point.registerStatusItem:(at) src/features/plugins/runtimeUi.tsx:164`               | re-keyed in parity.json                                                                                        |
| `point.registerTextEditorWrapper:(at) src/features/plugins/uiPoints.tsx:471`         | `point.registerTextEditorWrapper:(at) src/features/plugins/uiPoints.tsx:478`         | same registration moved from line 471 to 478                                                                   |
| `point.registerUndoHandler:(at) src/features/project-tree/index.ts:209`              | `point.registerUndoHandler:(at) src/features/project-tree/index.ts:242`              | re-keyed in parity.json                                                                                        |
| `settings:(at) src/features/plugins/settings.tsx:327`                                | `settings:(at) src/features/plugins/settings.tsx:376`                                | re-keyed in parity.json                                                                                        |
| `settings:(at) src/features/plugins/settings.tsx:337`                                | `settings:(at) src/features/plugins/settings.tsx:386`                                | re-keyed in parity.json                                                                                        |
| `settings:(at) src/features/settings/pages/registry.tsx:62`                          | `settings:(at) src/features/settings/pages/registry.tsx:64`                          | same registration moved from line 62 to 64                                                                     |
| `settings:(at) src/features/settings/pages/registry.tsx:69`                          | `settings:(at) src/features/settings/pages/registry.tsx:71`                          | same registration moved from line 69 to 71                                                                     |
| `settings:(at) src/features/settings/pages/registry.tsx:82`                          | `settings:(at) src/features/settings/pages/registry.tsx:84`                          | same registration moved from line 82 to 84                                                                     |
| `settings:languages.ruff`                                                            | `settings:python.ruff`                                                               | re-keyed in parity.json                                                                                        |
| `settings:tools.agents.chat`                                                         | `settings:agents.chat`                                                               | re-keyed in parity.json                                                                                        |
| `settings:tools.mcpServer`                                                           | `settings:agents.mcpServer`                                                          | re-keyed in parity.json                                                                                        |
| `unit:src/platform/filePresentation.cache.test.tsx`                                  | `unit:src/ui/filePresentation.cache.test.tsx`                                        | file renamed (src/platform/filePresentation.cache.test.tsx → src/ui/filePresentation.cache.test.tsx)           |
| `unit:src/platform/filePresentation.test.ts`                                         | `unit:src/ui/filePresentation.test.ts`                                               | re-keyed in parity.json                                                                                        |

## Keys of the old inventory with no row now (9)

- `boundid:navigate.relatedSymbol`
- `point.registerRunExecutor:profiler`
- `settings:languages.pluginFormatters`
- `settings:languages.serverTargets`
- `settings:projectStructure`
- `settings:tools.agents`
- `module:src/ui/InlineTag.tsx`
- `ipc:ssh_terminal_command`
- `unit:src/features/settings/__tests__/languageServerTargets.test.tsx`
