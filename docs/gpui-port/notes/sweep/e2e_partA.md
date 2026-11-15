Counting rule: a flow is proven by a test that runs in a project window (a `Workbench`, including the `editor::intelligence` fixture), or, when the spec itself mounts a lone component or harness page, by a test of the native component. Tests of the agent views mounted alone were not counted for the agent specs, which go through the workbench's docked conversation.

agentApproval | agents | yes (app-gpui/src/agents/session_view.rs:1886 permission card) | 0 of 1 | the spec is the card's geometry in the column at two widths and two themes; `workbench::tests::a_permission_request_shows_its_answers_and_the_one_clicked_reaches_the_agent` proves the answers, not the layout
agentMarkdown | agents | yes (agents/markdown_view.rs, agents/session_view.rs) | 0 of 12 | 9 cases are CSS metrics; copy/quote proven only on the session view mounted alone (`agents::session_view::tests::proofs8_agent_reply::copies_the_latest_streamed_text_and_clears_feedback_when_content_changes`, `agents::session_view::tests::quotes_and_regenerates_the_latest_reply`), and the quote's arrival in the draft is not asserted
agentScroll | agents | yes (agents/follow.rs, session_view.rs jump to latest) | 0 of 2 | scroll-geometry stability not measured; jump to latest proven only by click on the view alone (`agents::session_view::tests::offers_one_click_back_to_the_latest_message_after_scrolling_up`), no keyboard fold
agentsDock | agents | partial (agents/task_list_view.rs:32 DockTask event) | 0 of 3 | no test drags a session onto the editor or the tool window and reads the window; the view test only checks the emitted event
angularInline | editor highlighting | no (no Angular template mode found in app-gpui) | 0 of 1 |
attachProject | workspace | yes (app-gpui/src/workbench/app_shell.rs:903 OpenStep::Attach) | 0 of 3 | only `app_models::windows::tests::*` (flow model); no window test of attach/detach/restricted banner
boot | webview boot | no counterpart (JS module splash/timeouts) | 0 of 4 | not applicable natively
clickhouse-source | database | yes (database/vendors.rs:39) | 0 of 1 | `database::source_dialog::tests::switches_type_with_its_port_and_clickhouse_defaults` is a form model test; no dialog/Test Connection/transaction-control test in a window
codeInsight | code insight | yes | 3 of 7 | partial
commentToggle | editor | yes (comment commands) | 0 of 1 | JSX comment toggling proven only in `editor_commands::comments` model tests
commit | VCS commit | yes (workbench/commit.rs:45) | 1 flow minus the Unversioned default | partial
commitInclusion | VCS commit | yes | 1 of 1 | done
compactSettingsControls | settings layout | yes (settings pages) | 0 of 2 | layout metrics only
compareWith | diff | yes (project_view/context_menu.rs:511) | 2 of 4 | partial
configurationNavigation | editor LSP | partial (editor_lsp/configuration_navigation.rs) | 0 of 3 | request sharing only in model tests; harness metrics not reproduced
conflictMarkers | editor/VCS | model only (vcs_diff/conflict_markers.rs, used by nothing outside it) | 0 of 1 |
console-selection | run console | yes (run/view.rs) | 0 of 3 | `run::view::tests::select_all_copies_the_whole_buffer_and_the_menu_follows_the_input` copies a one-line buffer; nothing about selections across many rows
cornerInset | editor layout | not found | 0 of 1 | layout
databaseDialogLayout | database layout | yes (database/source_dialog.rs) | 0 of 1 | layout
debug | debugger | yes | 1 of 1 | done
deployment | deployment | model only (features_b::deployment is used by nothing outside features_b) | 0 of 4 |
diff-navigation | diff | yes (workbench/diff_window.rs:298) | 1 of 3 | partial
diff | diff viewer | yes (vcs_diff/view.rs) | 0 of 11 | content cases proven only on the DiffView mounted alone (`vcs_diff::view::tests::*`); moved-code view pending (proofs12_moved_code_view_pending.rs), soft wrap pending (proofs12_diff_soft_wraps_pending.rs), no 10,000-line timings
editor | editor | yes | 5 of 8 | partial
gitIgnore | VCS | yes (project_view/context_menu.rs:326) | write+refresh half of 1 | partial
gitLogBranchMenu | VCS log | yes (vcs/log_branches.rs) | 0 of 1 | no test of a linked repository's commit branch menu
gitLogCommitChecks | VCS log/GitHub | no (no commit checks in the log) | 0 of 1 |
gitLogScrollbar | VCS log layout | yes (vcs/graph) | 0 of 1 | scroll-drag painting not tested
goHighlight | highlighting | partial | 0 of 1 | `editor_model::semantic_tokens::tests::classifies_go_variable_modifiers` is a model test; no scheme colours read from an editor
hierarchyRefactor | refactoring | partial (editor_refactor/refactor_this offers rows) | 0 of 3 | no Pull Members Up / Push Members Down dialog flow in a window
httpHighlight | HTTP client | model only (features_a::http_client is not reached from workbench/editor) | 0 of 1 |
icls-php | themes | partial (theme/icls.rs) | 0 of 1 | `theme::icls::tests::preserves_php_colours_and_regular_styles_separately_from_java_and_language_defaults` is import model; no painted PHP editor
keymap | settings | yes | 1 flow, mouse half on the page alone | partial
kotlinHighlight | highlighting | yes | 1 of 2 | partial
linkedGradle | build tools | partial (platform/project_presentation) | 0 of 1 | no link/run/unlink flow in a window

Totals: 10 rows proposed (2 done: commitInclusion, debug; 8 partial), 25 omitted. Feature natively present (yes or partial) in 28 of 35; absent, not found or model only in 7 (angularInline, boot, conflictMarkers, deployment, gitLogCommitChecks, httpHighlight, cornerInset).
