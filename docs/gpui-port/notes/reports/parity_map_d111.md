# Parity map D111: rows earned since d468e8c7, and the e2e rows

The map (`parity_map_d111.json`, 42 rows) cites only tests in `rust_tests.txt`; new keys were checked against `INVENTORY.md`. The repository is unchanged.

## Counts by prefix

Task 1 (24 rows): `context` 1 done; `point.registerKeyClaim` 1 done; `point.registerOverlay` 4 partial; `point.registerToolWindow` 2 partial; `point.registerMenuGroup` 1 partial; `point.registerTitleItem` 1 partial; `action` 4 partial; `module` 3 partial; `event` 1 partial; `feature` 5 partial. That makes 2 done and 22 partial. Of these, 6 were blank: the diffDialog key claim, the `debugger` overlay, `vcs.gutter`, `intentions.ts`, `shellActions.ts` and `event:Debug`. The other 18 were partial already and get a corrected proof.

Task 2 (18 rows): `e2e` 2 done (commitInclusion, debug) and 16 partial.

## Task 1: rows added or moved

- `context:githubPrTab` becomes done. The tests now read `create` (proofs14 test line 134, part2 test line 892) as well as `list` and `pr`, which are all three values the webview publishes.
- `point.registerKeyClaim:(at) src/features/vcs/diffDialog.tsx:85` becomes done. The dialog view reads the keymap's binding for `toolwindow.closeContent` (`vcs_diff/dialog_view.rs:94`). `owns_the_close_shortcut_with_body_or_titlebar_focus` shows it closes the diff window whether the viewer or the window has focus, and the editor tab stays open.
- `point.registerOverlay:debugger` becomes partial: 4 of 6 overlays are proven (Evaluate, the Smart Step chooser, the Debug-mode configuration chooser, the Breakpoints dialog). The breakpoint properties popover and Attach to Process have no window test.
- `point.registerMenuGroup:vcs.gutter` becomes partial. A test reads Close Annotations and closes the column, but none reads the "Annotate with Git Blame" label.
- Out-of-date Missing lists corrected:
  - `pullRequests`: the New Pull Request tab and the + button now exist.
  - `project` tool window: the Options view options and the panes now exist.
  - `editor.pasteHistory`: mouse selection, double-click, focus return and reset are now proven.
  - `search.bookmarks`: the chooser now opens under the caret.
  - `run.widget`: Debug is offered.
  - `code.reformatDialog`: Optimize imports now runs in a window.
  - `code.generate`, `code.overrideMethods`: `apply_action` now reformats inserted lines; untested for these two.
  - `app.about` (action and overlay): the Keymap Reference link is now drawn.
  - `module:src/editor/lsp/format.ts`: Optimize imports and formatLines are now proven.
- New partial modules:
  - `intentions.ts`: showIntentions, applyQuickFix, the → controls and the indentation of inserted code are proven; the bulb, the preview and fixes away from the caret are not.
  - `shellActions.ts`: 6 of its 11 actions are done and 2 are partial.
- `event:Debug` becomes partial. The window routes it to the debugger driver (`workbench/mod.rs:1896`, `debugger/driver.rs:327`), and the debugging and evaluate tests read the resulting state. The cited tests do not assert the output, breakpoint-changed or hot-code-replace events.
- Feature rows with corrected Missing lists: `github` (Create, viewed, worktree), `vcs` (Annotate, Show History, Local History and Compare with Branch are registered, not "none registered"), `run` (context runs and debugging), `project-tree` (view modes, file commands) and `plugins` (tree views, editor tab entries, marketplace, Update All).

## Task 1: rows looked at and rejected

IPC rows:

- `gh_pr_create`: the webview never calls it. Create goes through `gh_api` (already done), and the native code does the same (`github_part2.rs:172`).
- `sdk_locate_tools` and `sdk_python_effective`: same functions as the adapters (`run_context.rs:206,222`; `sdk.rs:96,28`), but every run_context test runs a `#!` script, so no state comes from them.
- `project_current`: same function (`app_shell.rs:174`, `agents_views.rs:39`, `commands/app.rs:275`), but no test reads workspace members.
- `agent_commands`, `agent_unqueue`, `agent_send_queued_now`, `agent_stop_background`: called (`agents_views.rs:58`, `workbench/agents.rs:244-248`), but no window test reads their result.
- `debug_evaluate` and `debug_completions`: Evaluate calls `debug_evaluate_in` (already done), and the native code never calls completions.
- `doc_sync_from_disk`: Reload from Disk re-reads through `set_encoding` (`editor_core.rs:272`), a different call.
- `window_open_comparison`: the native diff is an in-window dialog.
- `vcs_log`: the New PR commit list uses it, but the row's Missing items (path filter, typed date range) are unchanged.
- `plugin_set_ui_locale`: native calls the host's `set_ui_locale` (`plugins.rs:178`), but its test file `proofs10_locale.rs` is not declared and is not in `rust_tests.txt`.
- `plugin_program`, `plugin_choose_program`, `plugin_stage_package` and `system_font_families`: the native code never calls them.
- The plugin update, consent and grant IPC rows (`plugin_market*`, `plugin_grant`, `plugin_install_dir`) were already done.

Context keys:

- `pasteHistoryListFocus` is unchanged: it is still on while the filter has the keyboard.
- `github.toggleViewed` is not a webview key.

Points:

- `registerStatusItem:app.lightEdit`: only a model test (`status_items::tests::light_edit_offers_…`) reads it.
- `registerMenuGroup:debugger.evaluate`: no test opens the editor menu while the program is paused.
- `registerFontZoomSurface` (terminal): the IDE zoom is a different mechanism, and the only test is the `terminal::keys` model test.

Modules: `src/ui/Tabs.tsx`, `src/ui/Dialog.tsx`, `src/ui/Popup.tsx` and `src/editor/core/controller.ts` are too broad: the recent changes touch a small slice of each.

Actions: every action a new test runs by a string id through `run_action` or `run` is recorded. A regex catches only those call shapes, so I also spot-checked `editor.pasteHistory.paste`/`.remove`, `github.pullRequest.refresh` and `terminal.closeSession`; their rows are current. 773aa0f4, a4232263, 2de87503, 37784b0a and 861001da change styling, fonts and the inspection script; I found no row candidate in their file lists.

## Task 2: the e2e split

Rule used for both halves: a flow counts only when a Rust test drives it through the application (a `Workbench`, including the `editor::intelligence` fixture). Component tests count only where the spec itself mounts a lone component or harness page: mergeEditor, kotlinHighlight and the diff harness. On that rule I dropped projectTree, which rests only on `project_tree::tests`, and cut newBranch to 1 of 4.

The six single-flow specs where parts are proven but no single test drives the whole flow are all kept as partial "0 of 1 flows", with the proven parts named: commit, gitIgnore, keymap, openProject, plugin-settings and pluginNotifications.

Native presence:

- **Present (yes or partial):** 53 of 70.
- **No counterpart or only a model (9):** angularInline, cornerInset, conflictMarkers, deployment, gitLogCommitChecks, httpHighlight, overviewLane, overviewLaneSelection, pluginDetails.
- **Webview-only (3):** boot, pageScroll, treeWidth.
- **Undetermined (5):** sdkFieldLayout, selectPopupLayout, settingsHeaderLayout, settingsListLayout, settingsPaneLayout. These are layout-only specs; part B did not establish a native file:line for them.

Proposed rows:

- **Done (2):** commitInclusion and debug. I read both specs; one test drives each flow.
- **Partial (16):**

| Spec                      | Flows       |
| ------------------------- | ----------- |
| codeInsight               | 3 of 7      |
| editor                    | 5 of 8      |
| compareWith               | 2 of 4      |
| diff-navigation           | 1 of 3      |
| kotlinHighlight           | 1 of 2      |
| mergeEditor               | 8 of 16     |
| newBranch                 | 1 of 4      |
| run                       | 2 of 4      |
| search                    | 3 of 4      |
| toolwindows               | 3 of 10     |
| the six single-flow specs | 0 of 1 each |

The other 52 specs are omitted. Either the feature is absent, or it is proven only by view-alone tests (agents, diff), by model tests (highlighting, plugins, updates, attach, configuration navigation), or by no geometry measurement (layouts, tooltips, log scrollbar). Reasons per spec: /private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/e2e_partA.md and /private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/e2e_partB.md.

## Wrong existing rows

No audited `done` row fails the standards: `debug_step_in_target(s)` (`driver.rs:697` against `commands/debug.rs:237`), `debugHotSwapBuilding` (read on and off at `proofs11_hotswap.rs:94-120`), `run.chooseDebugConfig`, `help.keymapReference` and `github.pullRequest.toggleViewed`.

The partial rows corrected under Task 1 have Missing lists that are no longer true. The worst is `run.widget`'s "Debug is not offered": `the_run_widgets_debug_button_debugs_the_selection` has existed since c451a994.

`code.implementMethods` (done) still says "Not asserted: a choice between several implement actions". `implement_methods_offers_every_implement_action_and_applies_the_chosen_one` now asserts it, so that note is out of date, but the status is right.
