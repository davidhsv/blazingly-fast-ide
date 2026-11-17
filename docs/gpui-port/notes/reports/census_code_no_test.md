# Rows whose native code exists and no test drives it, by native file

Each row: the key, the native `file:line`, the webview source, and what a test would have to drive and assert. Read at `21685f123`. A test written for one of these must reach the code through the app (a window, an action run through the registry, a view's event) and assert the effect; a model test or a fake seam in place of the backend call does not close the row.

## `app-gpui/src/debugger/driver.rs`

- `ipc:debug_attachable_processes`: native app-gpui/src/debugger/driver.rs:792-796; webview src/features/debugger/attachPopup.ts:21. no test drives the attach popup through a window to the real call (attachPopup.ts:21)

## `app-gpui/src/editor/intelligence.rs`

- `ipc:lsp_goto_implementation`: native app-gpui/src/editor/intelligence.rs:829; webview src/editor/lsp/navigation.ts:281. lowered from the helper's partial, by the rule applied to vcs_log_details_batch: native reaches Go to Implementation through the sibling lsp_goto with GotoKind::Implementation (editor/intelligence.rs:829), streams no partial results, and its one test asserts only the empty answer

## `app-gpui/src/editor/structure/commands.rs`

- `ipc:doc_ui_mode`: native app-gpui/src/editor/structure/commands.rs:311; webview src/editor/core/backend.ts:101. no native test asserts a framework mode (Angular) from doc_ui_mode (src/editor/core/backend.ts:101); Hypothesis: the editor's own mode on open (src/editor/core/backend.ts:480) and previews (src/editor/core/cm/preview.ts:27) are covered only if the structure Support answer drives them; check callers of Answer::Support in app-gpui/src/editor/structure/commands.rs

## `app-gpui/src/editor/view.rs`

- `ipc:doc_snapshot`: native app-gpui/src/editor/view.rs:1009-1010; app-gpui/src/workbench/editor_language.rs:1914; webview src/features/cucumber/index.ts:73; src/features/vcs/fileRevisions.ts:67; src/features/vcs/commitChecks.tsx:142/146/153; src/editor/core/backend.ts:92. behaviour reproduced through a different backend API (Documents::rope_snapshot); verdict kept low because no test was read

## `app-gpui/src/editor_lsp/configuration_navigation.rs`

- `event:LspRefresh`: native app-gpui/src/editor_lsp/configuration_navigation.rs:157; webview src/editor/lsp/configurationNavigation.ts:156. only the configuration-key cache refreshes on it, tested by a model test; the webview's availability back-off clear (src/editor/lsp/availability.ts:57) has no native listener

## `app-gpui/src/everywhere/structure_view.rs`

- `ipc:doc_open_virtual`: native app-gpui/src/everywhere/structure_view.rs:195-196; app-gpui/src/workbench/refactor_actions.rs:642-643; webview src/features/search/structure/store.ts:47; src/editor/core/backend.ts:85; src/editor/navigation/index.ts:34; src/editor/lsp/files.ts:48. no native test opens a virtual document through these paths (structure_view.rs:196, refactor_actions.rs:643); the editor opening a virtual document (src/editor/core/backend.ts:85) and navigation into one (src/editor/navigation/index.ts:34) do not call doc_open_virtual in native (Hypothesis: they go through another op; check `grep -rn virtual_input app-gpui/src` callers)

## `app-gpui/src/plugins.rs`

- `ipc:plugin_set_ui_locale`: native app-gpui/src/plugins.rs:178; webview src/features/app/locale.ts:20. A test for it exists in app-gpui/src/workbench/plugin_runtime/tests/proofs10_locale.rs (a_plugin_reads_the_ui_language_the_settings_choose) but that file is not declared in plugin_runtime.rs (mods at :1605-1612) and is not in rust_tests.txt, so it does not run.

## `app-gpui/src/run_testing/services.rs`

- `action:debug`: native app-gpui/src/run_testing/services.rs:692; webview src/features/run/services.tsx:624. the run service's Debug toolbar item runs run.debugConfig (workbench/services_window.rs:677); only model tests (services::model::tests) cover it, and RerunDebugger is not carried out (services_window.rs:676)

## `app-gpui/src/services/docker.rs`

- `action:refresh`: native app-gpui/src/services/docker.rs:1740; webview src/features/services/docker.tsx:215. the Docker node's Refresh toolbar item reads the connection again (snapshot carried out at workbench/services_window.rs:706); the window test runs the registry action docker.refresh and answers the snapshots itself, no test clicks the item

## `app-gpui/src/settings_view/pages/build_run_targets/sources.rs`

- `ipc:plugin_run_targets`: native app-gpui/src/settings_view/pages/build_run_targets/sources.rs:126; webview src/features/plugins/backend.ts:336. Found on a second pass: the first grep for run_targets::list missed the host-method call.

## `app-gpui/src/settings_view/pages/general_updates.rs`

- `event:UpdateStatusChanged`: native app-gpui/src/settings_view/pages/general_updates.rs:51; webview src/features/settings/pages/updates.tsx:18. the Updates page follows it, but the page is listed only while app.checkForUpdates is registered (workbench/settings_window.rs:121), which production never does; the one test registers a stand-in handler (workbench::tests::proofs19_settings_misc::updates_status_check_now_and_the_automatic_check_apply)

## `app-gpui/src/workbench/app_shell.rs`

- `ipc:workspace_attach`: native app-gpui/src/workbench/app_shell.rs:914; webview src/features/app/windows.ts:201. orphan test file proofs6_attach.rs would be the proof if wired

## `app-gpui/src/workbench/build_tools.rs`

- `event:ProjectRootsChanged`: native app-gpui/src/workbench/build_tools.rs:386; webview src/features/build-tools/index.ts:287. only the build tool window reloads on it; no test emits it; the webview's eight other listeners (project roots, presentation, plugin navigation, libraries, run, project tree, library sources) have none

## `app-gpui/src/workbench/context_menus.rs`

- `point.registerMenuGroup:debugger.evaluate`: native app-gpui/src/workbench/context_menus.rs:256; webview src/features/debugger/index.ts:99. Evaluate Expression while the session is paused; only the model test of group order (context_menus::tests::the_groups_of_a_menu_come_in_order_each_followed_by_a_separator) shows it, with facts set by hand
- `point.registerMenuGroup:http.new`: native app-gpui/src/workbench/context_menus.rs:260; webview src/features/http-client/index.ts:156. New › HTTP Request in a folder; no window test opens the New menu and finds it
- `point.registerMenuGroup:search.structure.selectIn`: native app-gpui/src/workbench/context_menus.rs:307; webview src/features/search/index.ts:202. Select In › Structure for the active editor's file; no test

## `app-gpui/src/workbench/database.rs`

- `ipc:db_databases`: native app-gpui/src/workbench/database.rs:188; webview src/features/database/store.ts:675. database::store::tests::offers_the_console_toolbar_its_targets_and_schemas (store.rs:3695) checks that the store emits DbOp::Databases: a model test, no backend
- `ipc:db_schema_objects`: native app-gpui/src/workbench/database.rs:190-194; webview src/features/database/store.ts:703. no test asserts the routines / sequences the call lists (store.ts:703)
- `ipc:db_set_ssh_secret`: native app-gpui/src/workbench/database.rs:175-178; issued; webview src/features/database/Dialogs.tsx:80; src/features/database/store.ts:396. no test drives SetSshSecret through the window to the backend (store.ts:396, Dialogs.tsx:80)
- `ipc:db_trust_ssh_host`: native app-gpui/src/workbench/database.rs:179; app-gpui/src/workbench/database.rs:1485-1487; webview src/features/database/SourceDialog.tsx:517; src/features/database/store.ts:497. no test reaches db::trust_ssh_host through the window (store.ts:497, SourceDialog.tsx:517)

## `app-gpui/src/workbench/log_window.rs`

- `ipc:vcs_log_details_batch`: native app-gpui/src/workbench/log_window.rs:411; app-gpui/src/workbench/log_actions.rs:816; app-gpui/src/workbench/log_actions.rs:496; webview src/features/vcs/logDetails.ts:59. behaviour reproduced through another backend function; verdict reflects that no test was confirmed, not that none exists. The webview's only route to details is the batch (src/features/vcs/logDetails.ts:59) Hypothesis: workbench::log_actions::tests::edit_commit_message_rewords_the_head_and_an_older_commit_and_undo_restores_it uses one-line messages, so message_truncated is false and vcs_log_details (log_actions.rs:816) is not reached; the cherry-pick/protected-branch tests may reach log_actions.rs:496 but I did not read them.

## `app-gpui/src/workbench/refactor_actions.rs`

- `ipc:syntax_expression_at`: native app-gpui/src/workbench/refactor_actions.rs:291; app-gpui/src/workbench/refactor_actions.rs:425; webview src/editor/structure/index.ts:92. kept at code-no-test. Measured by reading: Refactor This requests FactRequest::ExpressionAt unconditionally after the server answers (editor_refactor/refactor_this.rs:443), so the cited test reaches the real call; the Refactor menu path needs `tree` from syntax_support (workbench/refactor_actions.rs:253) and a caret selection (refactor_this.rs:569). Hypothesis: the .mock test language has no structure-service tree, so the expression answer is empty and apply_the_single_matching_action_or_offer_a_choice never takes MenuStep::ExpressionAt; check with ide_host::ops::syntax::syntax_support for the mock language id.

## `app-gpui/src/workbench/services_window.rs`

- `ipc:docker_snapshot`: native app-gpui/src/workbench/services_window.rs:706-708; webview src/features/services/store.ts:240. no test shows a tree read by ide_docker::snapshot (src/features/services/store.ts:240)
