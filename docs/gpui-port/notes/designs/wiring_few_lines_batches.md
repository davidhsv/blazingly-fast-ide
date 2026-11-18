# The "a few lines" wiring entries, checked at HEAD and batched

Source: the 103 entries of `docs/gpui-port/notes/wiring_work.md` sized "a few lines" (audit at `81efb75f7`). Checked at HEAD `00bd90824` (confirmed with `git rev-parse` at the end of the work; 8 commits touched `app-gpui/src` since the audit). Native code was read with `git show HEAD:` / `git grep … HEAD`; the webview was read from the working tree. Entry numbers E1…E103 follow the order of `grep -n "a few lines)" docs/gpui-port/notes/wiring_work.md`; the work-list line is given with each.

Method for "still unreached": every named type/function was grepped word-wise at HEAD in `app-gpui/src`, excluding its own file and files or folders named `test(s)`; the hits were then read to tell a production caller from a caller that is itself unreached or inside a `#[cfg(test)]` module. Line numbers are at HEAD; "about :N" means the quoted line was read in a range starting near N. A claim that rests on reading rather than on a grep or a read at HEAD is marked "hypothesis" with the command that settles it, or "per the audit, not re-read at HEAD".

Verdict counts: few lines 12 (one batch), already wired or superseded 4, needs a view 22, larger than a few lines without a new view 11, needs a backend call 4, not a wiring item 26, needs care 23, Windows-only 1. Total 103.

Two verdicts beyond the brief's four were needed: "larger" (no new view, but more than a call: a new event field set by many commands, a save-participant step behind 7 callers, a new completion source, state a menu does not keep) and "Windows-only" (a call whose effect no test on this platform can prove).

## Summary table

| E   | Work-list line | Entry (module)                                     | Verdict                                 | Batch |
| --- | -------------- | -------------------------------------------------- | --------------------------------------- | ----- |
| 1   | 23             | remote_dev/recents                                 | needs care (network)                    | –     |
| 2   | 24             | remote_dev/environment                             | needs care (network)                    | –     |
| 3   | 25             | remote_dev/model                                   | needs care (network)                    | –     |
| 4   | 26             | remote_dev/container                               | needs care (running, network)           | –     |
| 5   | 27             | viewers/file_kinds FileProbes                      | not a wiring item                       | –     |
| 6   | 39             | github/api timeline_items                          | needs care (network)                    | –     |
| 7   | 40             | github/gutter                                      | needs care (network)                    | –     |
| 8   | 41             | github/toolwindow timeline                         | needs care (network)                    | –     |
| 9   | 42             | github/data_model merge message                    | needs care (network)                    | –     |
| 10  | 43             | github/create RemoteBranchDialog                   | not a wiring item                       | –     |
| 11  | 44             | github/batches merge_batches                       | needs care (network)                    | –     |
| 12  | 45             | github/create reviewers                            | needs care (network)                    | –     |
| 13  | 62             | editor_lsp/file_references                         | larger                                  | –     |
| 14  | 63             | editor_lsp/format formats_on_save                  | larger                                  | –     |
| 15  | 64             | editor_lsp/format_guard (commit reformat)          | larger                                  | –     |
| 16  | 65             | editor_lsp/hover_view quick fix                    | few lines                               | 1     |
| 17  | 66             | editor_lsp/completion offers_paths                 | larger                                  | –     |
| 18  | 67             | editor_lsp/snippet plain preview                   | not a wiring item                       | –     |
| 19  | 85             | run_testing/comparison                             | needs a view                            | –     |
| 20  | 115            | platform_more/inspections profiles                 | needs a view                            | –     |
| 21  | 116            | platform/misc PlatformInfo::guessed                | not a wiring item                       | –     |
| 22  | 117            | platform_more/occurrences                          | not a wiring item                       | –     |
| 23  | 118            | platform_more/running_processes                    | not a wiring item                       | –     |
| 24  | 119            | platform/window_groups                             | needs a view                            | –     |
| 25  | 120            | platform/misc file_manager_path (WSL)              | not a wiring item                       | –     |
| 26  | 121            | platform_more/inspections apply_profile_mixed      | needs a view                            | –     |
| 27  | 122            | platform/misc ErrorReporter                        | not a wiring item                       | –     |
| 28  | 123            | platform/i18n set_locale                           | already wired                           | –     |
| 29  | 124            | platform/misc Download JDK                         | needs care (network, running)           | –     |
| 30  | 125            | platform_more/inspections ProfileWatch             | not a wiring item                       | –     |
| 31  | 126            | platform/i18n register_messages                    | not a wiring item                       | –     |
| 32  | 127            | platform/virtual_docs is_gradle_build_input        | needs care (trust, possibly running)    | –     |
| 33  | 142            | vcs_diff/gutter lanes and scroll marks             | needs a view                            | –     |
| 34  | 143            | vcs_ops/branch_widget_extensions                   | needs care (network)                    | –     |
| 35  | 144            | vcs_more/log_branch_memory                         | already wired                           | –     |
| 36  | 145            | vcs_more/log_search                                | needs a view                            | –     |
| 37  | 146            | vcs_more/log_presentation                          | needs a view                            | –     |
| 38  | 147            | vcs_ops/clone                                      | needs care (network, running)           | –     |
| 39  | 148            | vcs_ops/log_store refs_signature…                  | already wired                           | –     |
| 40  | 149            | vcs_diff/model stash_label                         | few lines                               | 1     |
| 41  | 150            | vcs_ops/change_ops drop_stash                      | needs care (deleting)                   | –     |
| 42  | 151            | vcs_more/diff_blame                                | needs a view                            | –     |
| 43  | 152            | vcs_ops/compare_paths directory diff               | needs a view                            | –     |
| 44  | 153            | vcs_more/merge_scroll ScrollEchoes                 | not a wiring item                       | –     |
| 45  | 154            | vcs_ops/remote_ui is_push_rejected                 | needs a view                            | –     |
| 46  | 155            | vcs_more/project_switch                            | not a wiring item                       | –     |
| 47  | 164            | features_a/todo preview                            | few lines                               | 1     |
| 48  | 165            | features_a/graphql ReplaceFileText                 | needs care (network)                    | –     |
| 49  | 176            | editor_model/document DocumentRefresh              | not a wiring item                       | –     |
| 50  | 177            | editor/nav_history remove_path                     | few lines                               | 1     |
| 51  | 178            | editor_commands/selection_marks                    | few lines                               | 1     |
| 52  | 179            | editor/nav_history is_navigation                   | larger                                  | –     |
| 53  | 188            | run_producers/stack_trace                          | needs a view                            | –     |
| 54  | 189            | run/store test_report_events                       | needs a view                            | –     |
| 55  | 190            | run_producers/external_tools                       | needs care (running)                    | –     |
| 56  | 191            | run/go_actions                                     | needs care (running)                    | –     |
| 57  | 192            | run_producers/service_order                        | not a wiring item                       | –     |
| 58  | 193            | run_producers/ruby_tests                           | needs a view                            | –     |
| 59  | 194            | run/executors                                      | needs care (running)                    | –     |
| 60  | 195            | run/env_dialog                                     | needs a view                            | –     |
| 61  | 196            | run/go_generate                                    | needs care (running)                    | –     |
| 62  | 197            | run/build_task                                     | not a wiring item                       | –     |
| 63  | 198            | run_producers/producers produce_file               | not a wiring item                       | –     |
| 64  | 208            | editor_refactor/occurrences expression_words       | larger                                  | –     |
| 65  | 216            | app_models/windows RemoveFromWorkspace             | needs a backend call                    | –     |
| 66  | 217            | app_models/windows split_dropped, tabs_to_close    | needs care (trust)                      | –     |
| 67  | 218            | app_models/cli_command                             | needs care (writes outside the project) | –     |
| 68  | 219            | app_models/project_actions rename_prompt           | needs a backend call                    | –     |
| 69  | 220            | app_models/project_actions ide_mode_toggle_visible | not a wiring item                       | –     |
| 70  | 237            | theme/themes startup_theme                         | not a wiring item                       | –     |
| 71  | 238            | theme/color_scheme scheme_after_theme_change       | few lines                               | 1     |
| 72  | 239            | theme/themes ThemePreview                          | larger                                  | –     |
| 73  | 240            | theme/themes is_builtin_theme…                     | not a wiring item                       | –     |
| 74  | 241            | theme/fonts finish_load                            | not a wiring item                       | –     |
| 75  | 247            | services/docker cross references                   | needs a view                            | –     |
| 76  | 256            | project_view Show Members                          | larger                                  | –     |
| 77  | 257            | project_view/operations revealable_path…           | not a wiring item                       | –     |
| 78  | 258            | project_view contributed roots                     | larger                                  | –     |
| 79  | 259            | project_view refresh_os_clipboard                  | few lines                               | 1     |
| 80  | 260            | project_view NewPopup                              | not a wiring item                       | –     |
| 81  | 261            | project_view safe_to_delete                        | not a wiring item                       | –     |
| 82  | 269            | debugger breakpoint_gutter document_changed        | few lines                               | 1     |
| 83  | 270            | debugger breakpoints map_lines                     | few lines (same call as E82)            | 1     |
| 84  | 271            | debugger run_to_cursor inlay                       | needs a view                            | –     |
| 85  | 272            | debugger hotswap editor menu                       | few lines                               | 1     |
| 86  | 277            | settings_model/runtime                             | already wired (superseded)              | –     |
| 87  | 278            | settings_model/file_types user_file_type           | larger                                  | –     |
| 88  | 279            | settings_model/staging Project Structure           | needs a view                            | –     |
| 89  | 280            | live templates contributed snippets                | needs a backend call                    | –     |
| 90  | 281            | settings_model/assign_shortcut                     | needs a view                            | –     |
| 91  | 282            | settings_model/file_types override_key             | needs a view                            | –     |
| 92  | 283            | settings_model/settings_window                     | not a wiring item                       | –     |
| 93  | 284            | file templates contributed                         | needs a backend call                    | –     |
| 94  | 293            | terminal/windows_pty                               | Windows-only                            | –     |
| 95  | 301            | everywhere/controller invalidate_endpoints         | few lines                               | 1     |
| 96  | 311            | find/scopes_dialog                                 | needs a view                            | –     |
| 97  | 312            | find/store PopupSize                               | needs a view                            | –     |
| 98  | 313            | find/session preview_replacement                   | few lines                               | 1     |
| 99  | 314            | find/search_keys                                   | not a wiring item                       | –     |
| 100 | 322            | web_view WebSurface                                | needs a view                            | –     |
| 101 | 326            | welcome_model startup reopen                       | needs care (trust)                      | –     |
| 102 | 327            | workbench/windows sorted_labels                    | not a wiring item                       | –     |
| 103 | 331            | ui/menu_aim                                        | larger                                  | –     |

## Batch 1: editor, debugger gutter, theme, stashes, Find, Search Everywhere, project view (12 items)

Only 12 entries survive as genuine "few lines", so they form one batch. Files to read: `workbench/editor_core.rs`, `workbench/project_files.rs`, `workbench/debug.rs`, `workbench/context_menus.rs`, `workbench/todo_window.rs`, `workbench/mod.rs` (window activation), `workbench/http_client.rs`, `editor/view.rs`, `editor/intelligence.rs`, `theme/appearance.rs`, `vcs/git_forms.rs`, `find/popup_view.rs`, `everywhere/view.rs`. Items are ordered by how directly they resolve a parity row whose proof says "nothing in the app reaches it": items 1 to 7 each name such a row, items 8 to 12 found none by text.

Rows this batch could raise: `unit:src/editor/core/model/editHistory.test.ts`, `unit:src/editor/core/cm/selectionMarks.test.ts`, `unit:src/features/todo/TodoView.test.tsx` (and possibly `unit:src/features/todo/PreviewEditor.test.tsx`), `unit:src/features/debugger/__tests__/breakpoints.test.ts`, `unit:src/features/debugger/__tests__/hotswap.test.ts`, `unit:src/features/vcs/__tests__/decorations.test.ts`, `module:src/platform/colorScheme.ts` (its Missing clause includes "switching the scheme back when the theme turns light or dark"). A row's proof may name more than this wiring; re-read it before changing a status.

### 1. Back/Forward and Last Edit Location forget deleted files (E50, line 177)

Anchor: `Workbench::close_removed_tabs` (`app-gpui/src/workbench/project_files.rs:876`, `fn close_removed_tabs(&mut self, removed: &[String], window: &mut Window, cx: &mut Context<Self>) {`), called after a successful delete at `project_files.rs:773` (`workbench.close_removed_tabs(std::slice::from_ref(path), window, cx);`). `NavigationHistory::remove_path` (`editor/nav_history.rs:138`) and `EditHistory::remove_path` (`editor/nav_history.rs:222`) have no caller (grep). At hand: `self.editor_core.nav` and `self.editor_core.edits` (fields at `workbench/editor_core.rs:38-39`). Not at hand: a folder delete; `remove_path` takes one path, so a deleted folder needs the history entries filtered with `is_same_or_inside`, which `close_removed_tabs` already uses for tabs.

Webview: when a file's tab input is removed, `src/editor/core/controller.ts:1920-1922` calls `this.nav.removePath(input.path)` and `this.edits.removePath(input.path)`: after a deletion, Back/Forward and Last Edit Location no longer step to the deleted file.

Test: combine `last_and_next_edit_location_walk_through_the_edits` (`workbench/editor_core.rs:572`) with the delete of `a_confirmed_delete_searches_deletes_and_reports_what_could_not_be_deleted` (`workbench/mod.rs:11526`): edit two files, delete one through the project view, run Last Edit Location and Back, assert the active editor stays on the surviving file.

Rows: `unit:src/editor/core/model/editHistory.test.ts` (proof names `editor_core.rs:156`, "no deletion handler").

### 2. Selected text stays visible inside a find match or usage highlight (E51, line 178)

Anchor: the per-line loop of the editor's paint in `editor/view.rs`: `backgrounds(true)` marks are filled (:2680), then `for selected in selections.iter().filter(|range| touches(range.start(), range.end())) {` fills `bg-selected` (:2686-2689), then the `backgrounds(false)` marks are painted over it (:2690-2699). `editor_commands::selection_marks::selected_ranges` (`editor_commands/selection_marks.rs:9`) has no caller. At hand: `selections`, `area`, `touches`, the theme. Not at hand: nothing for the paint; for the test, `PaintedLayer` (`editor/view.rs:141`) has only `CaretRow` and `Guides`, so a variant for the repainted selection must be added.

Webview: `src/editor/core/cm/selectionMarks.ts:28-45` decorates the selected ranges with `.cm-openidea-selected`, drawn in the inactive selection colour over mark backgrounds, so a selection inside a find match or usage background stays visible.

Test: copy the find setup of `workbench/tests/proofs_editor_search_find.rs`; select text inside a highlighted match; assert the editor's recorded paint order (`#[cfg(test)]` `paint_order`, `editor/view.rs:2608`, :2652) has the new selection layer after the mark layer.

Rows: `unit:src/editor/core/cm/selectionMarks.test.ts`.

### 3. The TODO preview is an editor that edits (E47, line 164)

Anchor: `Workbench::preview_todo` (`workbench/todo_window.rs:351`); the forcing line is `let info = ide_workspace::DocumentInfo { read_only: true, ..info };` (:375); a new `Editor::with_text` is made each time (about :385). `HeldPreviews`, `PreviewText`, `PreviewAttach` (`features_a/todo.rs`) have no caller. At hand: the host, the label, the opened `DocumentInfo`. Few lines: keep the document's read-only flag, and when the preview already shows the item's path, move its caret instead of making an editor. Larger and left out of this item: keeping a dirty preview alive after the preview moves to another file (the webview's `heldDirty`).

Webview: `src/features/todo/PreviewEditor.tsx:40-50` keeps dirty attachments in `heldDirty` after the view unmounts and gives them back by path; the pane is an editable editor of the item's file.

Test: a ready, undeclared one exists: `the_todo_preview_edits_the_file_and_moves_its_caret_to_another_item_of_the_same_file` (`workbench/tests/proofs16_todo_run_pending.rs:6`), which asserts the preview is not read-only and that the same editor shows the second item of the file. Declare it.

Rows: `unit:src/features/todo/TodoView.test.tsx` (proof names `todo_window.rs:375`); `unit:src/features/todo/PreviewEditor.test.tsx` is partial with no Missing clause, so re-read it.

### 4 and 5. Breakpoints move with edits (E82 line 269, E83 line 270)

Anchor: `Workbench::watch_debug_editor` (`workbench/debug.rs:783`); its subscription handles only `if let &EditorEvent::GutterPressed { line, shift, alt } = event {` (:785-788). `breakpoint_gutter::document_changed` (`debugger/editor/breakpoint_gutter.rs:143`), which calls `map_lines` and `BreakpointStore::set_file_breakpoints`, has no production caller (grep; `map_line_numbers` is otherwise used only in a test, `debugger/breakpoints/special.rs:587`). At hand: the editor, `EditorEvent::Edited { origin, changes }` (`editor/view.rs:305`), the debug slot. Not at hand: the text before the edit, which `document_changed` takes (`edit: (&Rope, &[TextChange], &Rope)`); `Edited` is emitted at one place (`editor/view.rs:1093`, `cx.emit(EditorEvent::Edited { origin, changes: changes.clone() });`), so adding the previous rope there is one field. `ChangeOrigin.remote` maps to `EditOrigin::External` (`editor/view.rs:294`); there is no native "mirrored" origin, so other editors of the document must count as non-writers (hypothesis: they receive `External`; settle by reading how a document edit reaches a second editor, `git grep -n "EditOrigin::External" HEAD -- app-gpui/src/editor/view.rs`).

Webview: `src/features/debugger/editor/breakpointGutter.ts:234-246`: on a document change of the originating editor (not a mirrored pane, and for a remote change only the writer) the file's breakpoints and special breakpoints are moved with `mapLines` and stored; other editors only redraw.

Test: copy `clicking_the_gutter_toggles_the_lines_breakpoint` (`workbench/debug.rs:1388`): set a breakpoint on line 5, type two newlines above it, assert the breakpoint is now listed on line 7 (as listed in `toggling_a_breakpoint_at_a_line_marks_it_persists_it_and_lists_it_in_the_breakpoints_dialog`, `debug.rs:1355`).

The right-click "breakpoint properties" half of E82 (`gutter_context_menu`, `breakpoint_gutter.rs:319`) is not in this item: no gutter right-click handler was found (`git grep -n "ContextMenu::Gutter" HEAD -- app-gpui/src/workbench` to locate one).

Rows: `unit:src/features/debugger/__tests__/breakpoints.test.ts` (proof names `debug.rs:785`, "subscribes only to GutterPressed").

### 6. Reload Changed Classes in the editor menu (E85, line 272)

Anchor: `GroupDecl { id: "debugger.hotswap", menus: &[ContextMenu::Editor], order: 0, rows: None },` (`workbench/context_menus.rs:76`). Its neighbour is the pattern: `GroupDecl { id: "debugger.evaluate", …, rows: Some(debugger_evaluate) }` (:74) with `fn debugger_evaluate(_: ContextMenu, _: &MenuGroupContext<'_>, facts: &GroupFacts)` (:256) reading `facts.debug_paused` (:187). At hand: the menu paths and `GroupFacts`. Not at hand: a fact computed from `DebugState`, which `hot_swap_menu_action(paths, state)` (`debugger/hotswap.rs:43`) needs; the facts builder must set it. The action is registered (`workbench/debug.rs:54`) and in the Run menu (`workbench/menu_spec.rs:588`).

Webview: `src/features/debugger/index.ts:104-108` registers the `debugger.hotswap` editor menu group with `hotSwapMenuItems(paths)` (`src/features/debugger/hotswap.ts:44-47`): one `.java` file and at least one session that can hot swap give the row "Reload Changed Classes".

Test: copy the session of `reload_changed_classes_compiles_then_reloads_the_classes_into_the_java_session` (`workbench/debug/tests/proofs11_hotswap.rs:89`) and the right-click of `right_click_in_a_plain_file_opens_the_editor_menu_and_moves_the_caret` (`workbench/context_menus.rs:1620`); assert the row is in the menu of the Java file during the session and absent without one.

Rows: `unit:src/features/debugger/__tests__/hotswap.test.ts` (proof names `context_menus.rs:469`).

### 7. Stash rows show the message and the branch apart (E40, line 149)

Anchor: the Unstash form's spec in `vcs/git_forms.rs` (about :1242-1247): `label: format!("stash@{{{}}}: {}", stash.index, stash.message), detail: String::new(),`. `vcs_diff::model::stash_label` has no caller. At hand: each `Stash`. Not at hand: nothing.

Webview: `src/features/vcs/CommitView.tsx:1398-1404` draws a stash as its message text (with the repository name when there are several), the branch apart, and the full `stash@{n}: message` as the tooltip.

Test: copy `stash_then_unstash_restores_the_change` (`workbench/vcs_operations.rs:1534`); stash with a message on a branch, open Unstash, assert the list row's label is the message and its detail the branch.

`progress_fraction` (same entry) has no native consumer found; leave it. Rows: `unit:src/features/vcs/__tests__/decorations.test.ts` (proof names `git_forms.rs:1263`).

### 8. The editor scheme follows a light/dark theme switch (E71, line 238)

Anchor: `theme::appearance::set_theme` (`theme/appearance.rs:240`, `pub fn set_theme(window: &str, next: ThemeSetting, cx: &mut App) -> anyhow::Result<()> {`), which writes only `theme_setting_writes(&next, sync)` and calls `apply(cx)`; and `system_appearance_changed` (`theme/appearance.rs:218`) for Sync with OS. `scheme_after_theme_change` (`theme/color_scheme.rs:587`) has no caller. At hand: the settings and `settings::set_global`. Not at hand: the kind before the switch; `apply` (`theme/appearance.rs:103`) recomputes the stylesheet from the settings and keeps no previous kind, so the caller must read `current_kind(cx)` before and after (used by `toggle_theme`, `appearance.rs:255`) and write `editor.colorScheme` when it changed.

Webview: `src/editor/core/colorSchemeRuntime.ts:34-50`: when the effective theme kind turns light or dark, a chosen editor scheme of the other kind is replaced by the theme default in the global settings.

Test: copy `the_theme_toggle_action_switches_and_stores_the_theme` (`workbench/mod.rs:4781`): choose a dark editor scheme, toggle to the light theme, assert the global `editor.colorScheme` is the theme default.

Rows: `module:src/platform/colorScheme.ts`.

### 9. Hover quick-fix link (E16, line 65)

Anchor: `EditorIntelligence::show_hover` (`editor/intelligence.rs:1128`, in `impl EditorIntelligence {` at :997), which calls `view.show(&popup, cx)` (:1145-1148); and the hover event match at `editor/intelligence.rs:1214`, `HoverEvent::QuickFix { .. } | HoverEvent::Link(_) => {}`. `HoverView::set_quick_fix` (`editor_lsp/hover_view.rs:360`) is called only from that file's tests. At hand: the editor, the shown problems, the Backend global, and the code-action request the editor already makes (`ide_host::ops::lsp::lsp_code_actions(&host, &window, doc, range, Vec::new())`, `editor/intelligence.rs:1335`). Not at hand: nothing; ask code actions for the problem's range after `show`, call `set_quick_fix` with the first, and apply it on `HoverEvent::QuickFix`.

Webview: `offerQuickFix` (`src/editor/lsp/hover.ts:122-148`): once a hovered problem shows, it asks the quick fixes and, if the server names one, inserts a link "<title> <shortcut>"; clicking it closes the hover and applies the action.

Test: copy the fake-server setup of `reformat_code_applies_the_servers_edits_and_reports_the_formatted_lines` (`workbench/editor_language.rs:2060`); give the server a diagnostic with one quick fix, hover the problem, assert the hover lines (`HoverView::lines`, `hover_view.rs:387`) include the fix title, click it, assert the document text has the fix.

Found on the way: `HoverEvent::Link` is also dropped at `intelligence.rs:1214`, so documentation links in a hover do nothing. The ⌘-hover declaration (`show_declaration`) is a separate path and not in this item. Rows: none found by text.

### 10. Search Everywhere asks for endpoints again when they change (E95, line 301)

Anchor: `Workbench::http_files_changed` (`workbench/http_client.rs:1228`, `self.http_client.providers.endpoints_changed();` at :1231) and the Spring route scan (`http_client.rs:1218`). `SeController::invalidate_endpoints` (`everywhere/controller.rs:305`) has no caller. At hand: the workbench and so its open Search Everywhere view. Not at hand: a method on the view that calls `self.controller.invalidate_endpoints()` (field at `everywhere/view.rs:70`) and runs the current request again.

Webview: `src/features/search/everywhere/SearchEverywhere.tsx:166-172`: on `onEndpointsChanged` the controller drops the endpoints section and the popup queries again, so endpoints added or removed while it is open appear.

Test: copy the popup setup of `labels_the_actions_tab_s_checkbox_and_assigns_a_shortcut_with_alt_enter` (`workbench/tests/proofs41_e2e_search_everywhere.rs:111`) and the Spring project of `open_in_http_client_appends_the_endpoint_to_the_generated_requests_scratch` (`workbench/http_client/tests.rs:417`); with the Endpoints tab open, add a mapping to the source and deliver an `IdeEvent::FsChanged` for it (routed to `http_files_changed` at `workbench/mod.rs:2139`), assert the new route is listed without reopening.

Rows: none found by text.

### 11. Find in Files shows the replacement inline (E98, line 313)

Anchor: the result row closure in `find/popup_view.rs` (about :2323-2335, `let ranges: &[(u32, u32)] = if stale { &[] } else { &row.ranges };`). `find::session::preview_replacement` is used only in a test (`find/replace_flow.rs:509`). At hand: the row, its ranges, `replace` mode, the popup's `replacement: Entity<TextField>` (`find/popup_view.rs:256`) and the query. Not at hand: nothing beyond reading the preserve-case option where the row is drawn.

Webview: `src/features/search/find/parts.tsx:44-55`: in Replace mode each match in a row is shown struck out, followed by its replacement (regex groups and preserve case applied).

Test: use the opener at `workbench/tests/proofs12_find_popup.rs:43` with `replace = true`, type a regex query and a replacement with a group, assert the row's text holds both the matched and the replaced text.

Rows: none found by text.

### 12. Paste follows files copied in the file manager (E79, line 259)

Anchor: the activation observer in `Workbench::new` (`workbench/mod.rs:1123`, `cx.observe_window_activation(window, |workbench, window, cx| {`). `project_view::operations::refresh_os_clipboard` (`project_view/operations.rs:1505`) has no caller; its request is already served (`Request::OsClipboardFiles =>` at `workbench/project_files.rs:835`, reading `ClipboardEntry::ExternalPaths` at :841). At hand: the workbench and `start_flow` (`project_files.rs:554`). Not at hand: nothing.

Webview: `src/features/project-tree/ProjectView.tsx:768-775`: on mount and on every window focus the tree re-reads the OS clipboard, so Paste is enabled for files copied in the file manager and an expired Copy/Cut is dropped.

Test: copy the project setup of `a_confirmed_delete_searches_deletes_and_reports_what_could_not_be_deleted` (`workbench/mod.rs:11526`); put an external-paths entry on the clipboard, activate the window, open the project view's context menu, assert Paste is enabled (built from `can_paste` at `project_view/context_menu.rs:406`). No existing test writes an `ExternalPaths` clipboard entry (grep finds only terminal drop tests, `terminal/panel/tests/proofs23_groups.rs:269`); whether the test platform keeps one is unverified (try it first; if it does not, the test proves the refresh through a project-view copy that expired).

Rows: none found by text (`unit:src/features/project-tree/operations.test.ts` is partial with no Missing clause).

## Already wired (or superseded) at HEAD

- E28 (line 123) `I18n::set_locale`: called by `platform::ui_language::apply` (`app-gpui/src/platform/ui_language.rs:56`, `if !localization.set_locale(Some(&stored), &system) {`), installed at start (`app-gpui/src/lib.rs:77`); landed in `1dae67072`.
- E35 (line 144) log branch memory: `remembered_log_branches` at `app-gpui/src/workbench/log_window.rs:259`, `remember_log_branches` at `log_window.rs:285`; landed in `c107abed7`.
- E39 (line 148) `refs_signature` at `workbench/log_window.rs:255`, `resolve_log_branches` at `log_window.rs:260`. `in_date_range` is still only in tests (`vcs_more/log_filters.rs:84`, `vcs_ops/log_store.rs:605`); the native log has no date filter to call it from.
- E86 (line 277) settings runtime: `runtime_settings` is used by idle saving (`workbench/idle_save.rs:20`), and the locale part the entry said was lost is applied by `platform/ui_language.rs:56`. `SettingsReload` and `RELOAD_DELAY_MS` stay unused (grep) because each feature listens to `SettingsChanged` itself.

## Needs a view

- E19 (line 85) failed-test comparison and E54 (line 189) JUnit report events: no test tree view exists (grep for `TestTree…View` in `app-gpui/src` finds none).
- E20 (line 115) inspection profiles: the Inspections page is registered as a bare spec (`settings_model/inspections_model.rs:632`, `PageSpec::new("editor.inspections", "Inspections", 25)…`) and no settings view draws its body (grep for `editor.inspections` in `app-gpui/src/settings_view` finds none). The audit's "page absent" is half right: it is listed but empty.
- E24 (line 119) window groups: no project-window tab bar (grep for `WindowGroupTabs|window_group_tabs` finds none).
- E26 (line 121) `apply_profile_mixed`: reached from `features_b/analysis/inspect.rs:195`, itself unreached; Analyze > Inspect Code has no results view (per the audit, not re-read at HEAD: `git grep -n "analysis::inspect" HEAD -- app-gpui/src/workbench`).
- E33 (line 142) gutter lanes: `scroll_marks` needs an overview strip in the editor; `ScrollbarMark` is used only by the diff and merge views (`vcs_diff/cm.rs:201`, `vcs_diff/merge_gutters.rs:8`).
- E36 (line 145) Git log speed search: `GitLog::key_down` (`vcs/log_view.rs:365`) ends `_ => return,`; wiring needs a query buffer with a reset timer and the small typed-text label the webview draws (`src/features/vcs/GitLog.tsx:1712-1714`), which no native view draws, and an adapter, since the log holds `Commit` (`vcs/log_view.rs:63`) while `next_speed_match` takes `RepoCommit`. Row: `unit:src/features/vcs/__tests__/logSearch.test.ts`.
- E37 (line 146) `present_log_graph`: reached only from `vcs_more/log_loading.rs:768`, which nothing outside tests reaches; the native log uses its own `present_graph` (`vcs/log_view.rs:343`) and ignores `collapse_linear` and `linearize_merges` (`vcs/log_options.rs:37`, :63). Collapse needs clickable collapsed-fragment rows.
- E42 (line 151) diff blame: no annotation gutter in diff panes. A ready, undeclared test exists for the merge view: `annotate_with_git_blame_is_offered_in_the_side_panes_only` (`workbench/tests/proofs_merge_annotate_pending.rs:10`).
- E43 (line 152) directory diff: no Comparison tree view (grep for `Comparison` in `workbench/diff_window.rs` finds none).
- E45 (line 154) `is_push_rejected`: used by `vcs_more/push_rejected.rs:161` and `vcs_more/workflow.rs:30`, neither reached; the native push reports "Push failed" (`workbench/branches.rs`, about :1050) with no push-rejected dialog.
- E53 (line 188) Analyze Stack Trace: `analyze.stackTrace` appears only in `workbench/menu_spec.rs:511` (grep); the dialog is missing.
- E58 (line 193) Ruby test navigation: `Resolve::RubyTest` targets come only from test results (`run_testing/ruby.rs:56`, `run_testing/results.rs:250`), so the test tree comes first.
- E60 (line 195) env table: `env_from_rows` is reached (`run/edit_configs_view.rs:964`); the table with Copy and paste-replace is not drawn.
- E75 (line 247) Docker cross-references and detail tabs: callers only inside `services/docker.rs` (grep).
- E84 (line 271) Run to Cursor inlay: `inlay_button` is named only in `debugger/editor/run_to_cursor.rs` (grep).
- E88 (line 279) Project Structure: `open_project_structure` is named only in `settings_model/staging.rs` (grep).
- E90 (line 281) Assign Shortcut: needs a keystroke dialog over the workbench; the recorder exists only inside the Keymap page (`settings_view/keymap.rs:297`); the webview registers an overlay for it (`src/features/settings/index.ts:140-145`).
- E91 (line 282) Override File Type: needs a language chooser, and the override must be read by language detection (hypothesis for the reader: `grep -rn fileTypeOverrides crates`).
- E96 (line 311) scope editor: `FindPopupEvent::EditScopes { .. } => {}` (`workbench/mod.rs:3124`).
- E97 (line 312) Find popup size: no resize grip or split handle (no `find-popup-resize` in `find/popup_view.rs`). A ready, undeclared test exists: `is_resizable_and_opens_at_the_size_it_was_left_at` (`workbench/tests/proofs12_find_popup_pending.rs:11`). Row: `unit:src/features/search/__tests__/findPopup.test.tsx` ("a resizable popup that remembers its size and its split").
- E100 (line 322) `WebSurface`: created only by `self_check.rs:184`.

## Larger than a few lines (no new view)

- E13 (line 62) and E17 (line 66) file-path completion: `offers_paths` (`editor_lsp/completion.rs:769`) gates a completion source the native app does not have; the webview's `pathFallbackSource` (`src/editor/lsp/completion.ts:879`) lists directory entries.
- E14 (line 63) format on save: `Editor::save_document` has 7 production callers (`editor/view.rs:3467`, `ui/code_preview.rs:185` and :536, `workbench/deployment.rs:1046`, `workbench/mod.rs:1862`, :2014, :2032, `workbench/plugin_contributions.rs:1763`), and it runs the cleanup synchronously before `write_as_is` (`editor/view/save_cleanup.rs:21-35`), while the format path is the workbench's async `format_document` (`workbench/editor_language.rs:425`) asked with `on_save: false` (:385). Wiring means a save-participant step every caller goes through. Webview: `registerFormatOnSave` (`src/editor/lsp/actions.ts:145-154`). Row: `unit:src/editor/lsp/format.test.ts`.
- E15 (line 64) commit-time reformat: the audit is half wrong. The commit box has the check (`vcs/commit_box.rs:84`, drawn at :947), but it is marked unavailable, "Needs a language server's code actions and formatting, which do not run before a commit here" (`workbench/shelf.rs:281-287`); wiring is a format-and-guard step over the committed files in `run_commit_checks` (`workbench/shelf.rs:2024`), as the webview does (`src/features/vcs/commitChecks.tsx:149-160`). It writes files before a commit, so handle with care.
- E52 (line 179) navigation cause of caret moves: `SelectionMoved` has no cause (`editor/view.rs:308`, emitted at :968); the workbench records every move with `navigation=false` (`workbench/editor_core.rs:160`). Tagging pointer, reveal, search and navigate moves is a field several editor commands must set.
- E64 (line 208) `expression_words`: serves the missing syntax-only Introduce Variable.
- E72 (line 239) theme preview: `ui/popup_menu.rs` has no active-row callback (grep for `on_highlight|on_active|highlighted` empty), and the theme applies to every window.
- E76 (line 256) Show Members: file rows must become expandable and ask an outline per file (`project_view/store.rs:653` only checks the option); `member_nodes` (`project_tree_model.rs:428`) has no caller.
- E78 (line 258) contributed roots: `register_root` is called only in a test (`project_view/operations.rs:2775`).
- E87 (line 278) user File Types icons: `file_icon_name(file_name)` (`theme/icons.rs:126`) has no settings or `App` and 33 callers (grep).
- E103 (line 331) submenu aiming: the row's `on_mouse_move` discards the event (`ui/popup_menu.rs:430`), and the menu keeps no level bounds (`PopupMenu` holds only `position`, :181) nor pointer history; the webview also defers the hover 300 ms while aiming (`src/ui/Menu.tsx:240-253`). Rows: `unit:src/ui/menuAim.test.ts`, `module:src/ui/menuAim.ts`.

## Needs a backend call the native app does not make

- E65 (line 216) Remove from Workspace / Detach: `RemoveStep::Update { name, projects }` (`app_models/windows.rs:652`) needs `ide_host::ops::project::save_workspace` (`crates/ide-host/src/ops/project.rs:81`), which `app-gpui/src` never calls (grep); the menu offers the actions (`project_view/context_menu.rs:388`) with no handler. Small once the call is made.
- E68 (line 219) Rename Project: the webview calls `commands.projectRename` (`src/features/app/projectActions.ts:159`), implemented only as the Tauri command `src-tauri/src/commands/app.rs:157` (writes `general.projectName` or calls `save_workspace`, records the project, sets the title). There is no `ide_host` op, so the native app has to compose it. Row: `unit:src/features/app/__tests__/projectName.test.ts`.
- E89 (line 280) and E93 (line 284) plugin snippets and file templates: the webview projects the plugins' `snippets` and `fileTemplates` UI points (`src/features/plugins/uiPoints.tsx:513-535`); the native app reads neither (only `plugin_models/plugin_text.rs:122-123` names them, for translations). Row: `unit:src/features/templates/__tests__/views.test.tsx`.

## Not a wiring item

Each line gives the HEAD evidence that the behaviour is reached another way, or says the claim is the audit's.

- E5 (line 27) `FileProbes`: probing is reached; only de-duplicating concurrent probes is missing (per the audit, not re-read at HEAD: `git show HEAD:app-gpui/src/workbench/tab_views.rs | grep -n probe`).
- E10 (line 43) `RemoteBranchDialog`: name asked through a prompt and cleaned after OK (per the audit: `git show HEAD:app-gpui/src/workbench/github_part2.rs | sed -n 285,300p`).
- E18 (line 67) snippet plain preview: peripheral (per the audit: `git grep -n "lsp_snippet_to_template" HEAD -- app-gpui/src | grep -v test`).
- E21 (line 116) `PlatformInfo::guessed`: a browser-user-agent guess; `guess_os` and `with_app_info` have no caller (grep) and the native app is compiled per OS (`workbench/shelf.rs:270-277` `this_os` uses `cfg!`).
- E22 (line 117) occurrence navigators: Next/Previous Occurrence is `Workbench::go_to_occurrence` (`workbench/editor_extras.rs:204`).
- E23 (line 118) running-process providers: used only in `app_models/close_guard.rs` tests (:485, :767).
- E25 (line 120) WSL file-manager paths: `file_manager_path` is called only from `project_view/operations.rs:1851`, inside the unreached `revealable_path`; the native app runs no WSL session.
- E27 (line 122) `ErrorReporter`: its user `platform/error_log_sink.rs` is not declared in `platform/mod.rs` (grep count 0); GPUI has no uncaught-error event to call it from, so wiring means choosing report sites, not adding a missing call.
- E30 (line 125) `ProfileWatch`: settings changes re-apply the profile through `code_insight_settings_changed` (`workbench/code_insight.rs:456`).
- E31 (line 126) `register_messages`: the webview's `registerMessages` (`src/platform/i18n.ts:129`) has no production caller either.
- E44 (line 153) `ScrollEchoes`: synchronized scrolling goes through `MergeView::set_scroll` (`vcs/merge_view.rs:1321`), applied synchronously (hypothesis that no echo arrives: read `set_scroll`).
- E46 (line 155) `reset_git_log`: the log lives in the workbench's `git_log: Option<log_window::LogSlot>` (`workbench/mod.rs:384`), dropped with the project (per the audit, `workbench/mod.rs` about :3344).
- E49 (line 176) `DocumentRefresh`: `Editor::refresh_highlights` (`editor/view.rs:1625`) refreshes folds (`core_actions::refresh_folds`, :1630) and debounces highlights with `HIGHLIGHT_DELAY` (:1636).
- E57 (line 192) `ServiceOrders`: the order is kept by `run_testing/services.rs` (`order_services` :106, `move_service_order` :120).
- E62 (line 197) `build_task`: `build_outcome` is reached (`run/driver.rs:900`); `begin_build` is reimplemented by `RunDriver::build`.
- E63 (line 198) `produce_file`: `produce_context` (`run_producers/producers.rs:1212`) covers it.
- E69 (line 220) `ide_mode_toggle_visible`: the chrome keeps its own `ide_mode` state (`workbench/chrome.rs:96`); per the audit the same rule is applied at `chrome.rs:317`, with a window test `proofs13_ide_mode`.
- E70 (line 237) start-up theme record: GPUI draws its own window background (per the audit; `startup_theme` has a separate definition in `plugin_models/startup_theme.rs:168`).
- E73 (line 240) theme id helpers: the theme is chosen in `theme/appearance.rs` (`sync_with_os` at :74, `set_theme` at :240).
- E74 (line 241) `SystemFonts::finish_load`: the native read returns a list and cannot fail (`theme/installed_fonts.rs:42`).
- E77 (line 257) `revealable_path` and notifications: WSL translation does not apply natively; `begin_rename` is used only in tests (`project_view/context_menu.rs:690`).
- E80 (line 260) `NewPopup` counter: a `NewPopupView` is made per open (`workbench/project_files.rs:47` imports it; per the audit at :988).
- E81 (line 261) `safe_to_delete`: Delete goes through `delete_selection` (`project_view/operations.rs:1238`).
- E92 (line 283) standalone Settings window start-up: the native Settings dialog opens in the workbench (`workbench/settings_window.rs:6`).
- E99 (line 314) `claims_search_key`: the popup claims keys with `claim_key` (`everywhere/view.rs:534`).
- E102 (line 327) `sorted_labels`: `order_key` (`workbench/windows.rs:16`) sorts the windows inline.

## Needs care (trust, secrets, network, running a program, deleting files)

- E1 (line 23), E2 (line 24), E3 (line 25): remote development (recent remote projects, Reload Environment, the remote status item): network sessions; E3's `browsable_folders`/`listed_directory` are used by `features_b/remote_dev/connect_dialog.rs:286-288`, itself unreached. Row: `unit:src/features/remote-dev/ConnectDialog.test.tsx`.
- E4 (line 26) Reopen in Container: runs a container and connects to it. Row: `unit:src/features/remote-dev/container.test.ts`.
- E6 (line 39), E7 (line 40), E8 (line 41), E9 (line 42), E11 (line 44), E12 (line 45), E34 (line 143): GitHub pull requests (timeline, review gutter, merge message, reviewers, review mode on the branch widget): network and a token. E7's `GutterState`/`PaneReview` are used by `github/review_diff.rs:19` and `github/review_editor.rs:22`, unreached; E11's `merge_batches` by `github/repository_data.rs:349`. A ready, undeclared test exists for the timeline: `registers_the_window_the_timeline_editor_and_the_actions` (`workbench/github/tests/proofs14_github_window_pending.rs:6`). Row: `unit:src/features/github/diff/model.test.ts`.
- E29 (line 124) Download JDK: network download and install.
- E32 (line 127) Gradle build inputs: the call is a few lines in `changes_build_model` (`platform/project_presentation.rs:159`, reached from `workbench/build_tools.rs:384`), and the reload it triggers is `ide_host::ops::build::build_model` (`workbench/build_tools.rs:406`), which requires trust (`crates/ide-host/src/ops/build.rs:140`). Whether loading the model runs Gradle is unverified (`crates/ide-host/src/ops/build/workspace.rs:99` has no `Command::new`; follow `load_scopes`). The webview also counts `mvnw`, `gradlew`, `build.sbt`, `project/build.properties` (`src/features/build-tools/index.ts:68-70`). Rows: `unit:src/platform/buildFiles.test.ts`, `module:src/platform/buildFiles.ts`.
- E38 (line 147) Clone Repository: network and git, plus a dialog.
- E41 (line 150) Drop Stash and the delete-files confirmation: deletes a stash and files.
- E48 (line 165) GraphQL introspection: network. Row: `unit:src/features/graphql/store.test.ts`.
- E55 (line 190) External Tools: runs user-configured programs.
- E56 (line 191), E61 (line 196) Go module actions and Go Generate: the menu group is `rows: None` (`workbench/context_menus.rs:88-93`) and no `go.mod.*` / `go.generate.*` action is registered (grep finds the ids only in `run/go_actions.rs` and `run/go_generate.rs`); they run `go`.
- E59 (line 194) run with an executor (coverage, profiler): runs programs.
- E66 (line 217) OS file drops and Close Project Tabs: no `on_drop::<ExternalPaths>` in the workbench (grep); a dropped folder opens a project (trust). The tabs half needs the window-groups view (E24). Row: `unit:src/features/app/__tests__/windows.test.ts` ("Missing: file drops …").
- E67 (line 218) Install/Uninstall the `openidea` command: writes a command link outside the project.
- E101 (line 326) reopen the last project at start-up: `main` opens `Target::Empty` (`app-gpui/src/main.rs:31`); `startup_project`/`reopen_enabled` (`workbench/welcome_model.rs:582`, :537) have no caller, and `SessionMemory::project_changed` is never called, so the last project is not even recorded (only the preference key `REOPEN_PREFERENCE`, `workbench/mod.rs:514`). It opens a project without a user action, through `open_project_asking` (`workbench/windows.rs:83`), so trust is involved. Rows: `unit:src/features/welcome/session.test.ts`, `unit:src/features/welcome/recent.test.ts`.

## Windows-only

- E94 (line 293) ConPTY hint: `windows_pty` is called only from `features_b/terminal.rs:190`, inside `LiveSession`, which nothing outside that file names (grep); the terminal builds `Emulator::new(cols, rows)` (`terminal/view.rs:197`). A few lines, but no test on macOS or Linux proves the effect. Row: `unit:src/features/terminal/windowsPty.test.ts`.

## Not checked

Every entry has a verdict. Where a verdict rests on the audit's text rather than a read at HEAD, the line says so and gives the command; those are E5, E10, E18, E26, E46 (partly), E69 (partly), E70, E80 (partly). Treat them as unchecked until the command is run.

## What in the audit turned out wrong or stale

- Four entries are wired or superseded since the audit (E28, E35, E39, E86).
- Most "a few lines" entries are not a call: 22 need a view, 11 need more than a call, 4 need a backend call, and 26 are not wiring items, most of them by the audit's own effect text ("Nothing lost", "None").
- E14 is not one call: `Editor::save_document` has 7 production callers, so format on save needs a save-participant step.
- E15 is half wrong: the commit box draws a "Reformat code" check, marked unavailable at `workbench/shelf.rs:281-287`.
- E20 is half wrong: the Inspections page is registered (`settings_model/inspections_model.rs:632`) but has no body.
- Several symbols the audit called unreached have callers that are themselves unreached (E3 connect dialog, E7 review diff and editor, E11 repository data, E26 inspect, E37 log loading, E45 push rejected, E94 LiveSession); a text match would wrongly count them as wired.
- E31's webview side is not wired either.
- E33 is more than peripheral: scrollbar marks need a strip the editor does not draw.
- E58 depends on the test tree, not on navigation code.
- Ready, undeclared window tests (`*_pending.rs`) exist for E47, E97, E42 (merge view) and the GitHub timeline (E6/E8); the work list does not mention them.
- Line numbers have drifted, for example `vcs/log_view.rs:351` is now `key_down` at :365 and the find popup's `EditScopes` arm is at `workbench/mod.rs:3124`, not :3044.
