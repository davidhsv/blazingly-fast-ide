# proofs5: registered actions whose row is not at parity (`docs/gpui-port/notes/sweep/registered_not_done.md`, 198 ids)

Drafted without a compiler. No existing file was edited. Paths are relative to the worktree.

## `mod` lines to add

1. `app-gpui/src/workbench/mod.rs`, inside `mod tests {` (the block that starts `#[cfg(test)]` / `mod tests {` / `use super::*;`), next to the anchor line `    mod proofs_vcs;` (cargo fmt sorts the block):

```rust
    mod proofs5_database;
    mod proofs5_layout;
    mod proofs5_navigation;
    mod proofs5_run;
```

2. `app-gpui/src/workbench/editor_language.rs`, inside its inline `mod tests {` (line ~1547), after the anchor line `    use std::path::Path;`:

```rust
    mod proofs5_editor;
```

## Files

- `app-gpui/src/workbench/tests/proofs5_navigation.rs` (2 tests)
- `app-gpui/src/workbench/tests/proofs5_database.rs` (1 test)
- `app-gpui/src/workbench/tests/proofs5_run.rs` (1 test)
- `app-gpui/src/workbench/tests/proofs5_layout.rs` (2 tests)
- `app-gpui/src/workbench/editor_language/tests/proofs5_editor.rs` (7 tests)

All helpers used exist: `start`, `settle`, `search_project`, `trusted_project`, `run_project`, `program`, `run_action`, `wait_long`, `context_is_on`, `sqlite_source`, `db_query`, `db_tree`, `db_row_key`, `db_expand` (workbench `mod tests`); `open`, `Fixture::{run, run_with, answer, requests, editing, text, active, put_caret, hint, wait_hint, notices, close}`, `edit`, `text_edit`, `range`, `uri` (editor_language `mod tests`). Accessors: `Workbench::{bookmarks, popup (field), editor, tool_windows, background_tasks_popup, tool_window_view, language_popup, paste_history_dialog}`, `BookmarkStore::data`, `MnemonicChooser::{title, current_mnemonic}`, `ToolWindowLayout::maximized`, `Editor::{selections, offset_of, select}`, `VisualTestContext::{opened_url, debug_bounds}`. No new accessor is needed.

## Per test → rows

### `workbench::tests::proofs5_navigation`

- `toggle_file_bookmark_without_targets_bookmarks_the_active_editors_file` → `action:bookmarks.toggleFileBookmark` → **done**. Proof: "`workbench::tests::proofs5_navigation::toggle_file_bookmark_without_targets_bookmarks_the_active_editors_file`, `workbench::context_menus::tests::the_tab_menus_bookmark_row_bookmarks_the_file_of_that_tab`, `everywhere::bookmarks_store::tests`; run through the registry without targets it bookmarks the active editor's file (nothing without an editor) and stores it with the project, with the targets the Project view's and tab's menus give it bookmarks a folder or the tab's file, and toggled again on a bookmarked file it removes that bookmark". (The Project view's row is `context_menus.rs:284`, the same `{ targets }` argument the test passes.)
- `toggle_bookmark_with_mnemonic_gives_the_caret_line_the_typed_digit` → `action:navigate.toggleBookmarkWithMnemonic` → stays **partial**. Proof: "`workbench::tests::proofs5_navigation::toggle_bookmark_with_mnemonic_gives_the_caret_line_the_typed_digit`, `everywhere::bookmarks_popup::tests`; ⌥F3 and the registry open the chooser for the caret's line, the digit typed gives the line a bookmark with that mnemonic and closes it, on that line it offers Change Mnemonic; without an editor the action does not run and ⌥F3 opens nothing. Missing: the chooser shows in the popup slot instead of at the caret".

### `workbench::tests::proofs5_database`

- `refresh_in_the_tree_lists_a_table_created_since_it_was_read` → `action:db.refresh` → **done**. Proof: "`workbench::tests::proofs5_database::refresh_in_the_tree_lists_a_table_created_since_it_was_read`; ⌘R with the tables folder selected in the focused tree reads the tables again and a table created since shows, the tables read before stay; ⌘R in a text editor (databaseViewFocus off) does not refresh the tree".

### `workbench::tests::proofs5_run`

- `open_in_browser_opens_the_started_servers_page_and_remove_from_services_hides_a_row` →
  - `action:run.services.openBrowser` (INVENTORY row exists unticked, `parity.json` has no entry: add one; same key shape as the done `action:run.services.runAll`) → **done**. Proof: "`workbench::tests::proofs5_run::open_in_browser_opens_the_started_servers_page_and_remove_from_services_hides_a_row`, `run_testing::services::tests::opens_the_known_group_url_and_offers_a_named_choice_for_several_web_apps`; run through the registry on a type group it opens nothing while no server runs and http://localhost:<port> once its started server reported the port".
  - `action:open-browser` (today **partial**: "it is never run, so opening the URL is not shown") → **done**. Proof: "`workbench::tests::proofs_services::run_all_and_stop_all_drive_a_type_group_and_a_started_servers_port_shows`, `workbench::tests::proofs5_run::open_in_browser_opens_the_started_servers_page_and_remove_from_services_hides_a_row`; the toolbar's Open in Browser is enabled while a server with a port runs and disabled after Stop; run through run.services.openBrowser on the group it opens http://localhost:8123, and nothing while no server runs".
  - `action:run.services.hideConfig` (INVENTORY row exists unticked, no `parity.json` entry: add one) → **done**. Proof: "same test; run through the registry with a configuration id it removes that configuration's row from the Services tree and leaves the other configuration and the group".

### `workbench::tests::proofs5_layout`

- `maximize_tool_window_draws_the_focused_side_across_the_window` → `action:toolwindow.maximize` → **done**. Proof: "`workbench::tests::proofs5_layout::maximize_tool_window_draws_the_focused_side_across_the_window`, `workbench::tests::appearance_toggles_hide_the_bars_and_tool_windows_maximize_and_resize`; run through the registry on the focused Project view its header is drawn across the window (its end past three quarters of the width, from under half), ⌘⇧' draws it back at its width; with the editor focused the action does not run".
- `background_tasks_opens_the_popup_and_running_it_again_keeps_it_open` → `action:view.backgroundTasks` → **expected to FAIL** today (the second run closes the popup); **done** after the fix below. Proof after the fix: "`workbench::tests::proofs5_layout::background_tasks_opens_the_popup_and_running_it_again_keeps_it_open`, `workbench::plugin_contributions::tests::cancelling_a_plugins_task_from_the_background_tasks_popup_stops_it`; the action opens the popup and, run again, keeps it open; the tasks status item toggles it and the popup lists the plugin's task with its working Cancel".

### `workbench::editor_language::tests::proofs5_editor`

- `code_cleanup_in_file_leaves_out_the_steps_its_argument_turns_off` → `action:code.cleanupFile` → stays **partial**. Proof: "`workbench::editor_language::tests::optimize_imports_and_code_cleanup_apply_the_servers_source_actions`, `workbench::editor_language::tests::proofs5_editor::code_cleanup_in_file_leaves_out_the_steps_its_argument_turns_off`; with { path } it opens the file, applies the server's fix-all and organize-imports actions and reformats it; `optimizeImports: false` and `reformat: false` leave the imports in their order and the file unformatted. Missing: its reformat step never uses a plugin-contributed formatter".
- `quick_type_definition_shows_the_type_definitions_lines_and_f4_opens_it` → `action:code.quickTypeDefinition` → stays **partial**. Proof: "`workbench::editor_language::tests::quick_definition_shows_the_definitions_lines_and_f4_opens_it`, `workbench::editor_language::tests::proofs5_editor::quick_type_definition_shows_the_type_definitions_lines_and_f4_opens_it`; run through the registry it shows the found type definition's file, line and lines in a popup with the keyboard, F4 opens it with the caret on it, and with none it says No type definition found. Missing: the popup shows a window of lines around the name instead of the declaration's whole span from the syntax tree, without syntax highlighting".
- The five hint tests below are **expected to FAIL** today: the native hint is a notification (`Workbench::show_hint`, `workbench/editor_core.rs:241`) or only logged (`editor/command_actions.rs:207,240`). They pass after the hint fix below.
  - `show_editor_hint_shows_the_text_at_the_caret_and_the_next_hint_replaces_it` → `action:editor.showHint` → **partial** after the fix. Proof: "`workbench::editor_language::tests::proofs5_editor::show_editor_hint_shows_the_text_at_the_caret_and_the_next_hint_replaces_it`; run with { text } it shows the text as the hint at the caret of the active editor, not as a notification, and the next hint replaces it. Missing: the tone it is given is passed on but no test reads it".
  - `the_copy_path_actions_say_what_they_copied_at_the_caret` → `action:editor.copyPath` → **done** after the fix; `action:editor.copyPathWithLineNumber` → **done** after the fix. Proofs: keep their current first sentences and replace the "Missing: … notification" clause with "`…proofs5_editor::the_copy_path_actions_say_what_they_copied_at_the_caret`: Copied <path> (and Copied src/a.txt:2 for the caret's line) shows as the hint at the caret".
  - `paste_from_history_with_an_empty_history_says_so_at_the_caret` → `action:code.pasteFromHistory` → stays **partial** after the fix (the dialog lacks parts of the webview's; see the editor.pasteHistory overlay). Proof: replace "the empty-history message is a notification, not the hint at the caret, and" with "⇧⌘V with an empty history says so in the hint at the caret (`…proofs5_editor::paste_from_history_with_an_empty_history_says_so_at_the_caret`). Missing:".
  - `ctrl_g_past_the_last_occurrence_says_not_found_then_searches_from_the_top` → `action:code.addCaretNextOccurrence` → **done** after the fix. Proof: "`editor::command_actions::tests::caret_and_occurrence_commands_run_through_the_registry`, `workbench::tests::the_find_bar_marks_its_occurrences_replaces_in_one_step_and_closes_on_escape`, `workbench::editor_language::tests::proofs5_editor::ctrl_g_past_the_last_occurrence_says_not_found_then_searches_from_the_top`; past the last occurrence ⌃G says Not found, press again to search from the top at the caret and adds nothing, and again adds the first occurrence".
  - `cyclic_expand_word_with_nothing_to_expand_says_so_at_the_caret` → `action:code.cyclicExpandWord` and `action:code.cyclicExpandWordBackward` → **done** after the fix. Proof: "`editor::command_actions::tests::caret_and_occurrence_commands_run_through_the_registry`, `workbench::editor_language::tests::proofs5_editor::cyclic_expand_word_with_nothing_to_expand_says_so_at_the_caret`; ⌥/ on a word nothing completes says No suitable completions found at the caret and leaves the text; Cyclic Expand Word Backward through the registry with no word before the caret says No word to expand at the caret".

## Fixes the failing tests need (exact hunks)

### 1. The editor hint is the hint at the caret (editor.showHint, copy path, Paste from History, ⌃G, ⌥/)

`app-gpui/src/workbench/editor_core.rs` (webview: `src/editor/lsp/actions.ts:353` shows `{ text, tone }` at the caret of the active editor and nothing without one):

```diff
@@ imports
+use crate::editor::intelligence::EditorIntelligence;
+use crate::editor_lsp::hint::Tone;
@@
-const HINT_KEY: &str = "editor.hint";
@@ register
     registry.handle("editor.showHint", |call, window, cx| {
         let text = call.arg.and_then(|arg| arg.get("text")).and_then(Value::as_str).unwrap_or_default().to_owned();
-        on_workbench(window, cx, move |workbench, _, cx| workbench.show_hint(text, cx))
+        let tone = match call.arg.and_then(|arg| arg.get("tone")).and_then(Value::as_str) {
+            Some("warning") => Tone::Warning,
+            Some("error") => Tone::Error,
+            _ => Tone::Info,
+        };
+        on_workbench(window, cx, move |workbench, _, cx| workbench.show_hint_toned(text, tone, cx))
     });
@@ impl Workbench
-    /// The editor hint: a short message about what an editor command did or could not do. It shows as
-    /// a notification, which the next hint replaces.
-    pub fn show_hint(&mut self, text: String, cx: &mut Context<Self>) {
-        if text.is_empty() {
-            return;
-        }
-        let request = NotificationRequest {
-            key: Some(HINT_KEY.to_owned()),
-            ..NotificationRequest::new(NotificationLevel::Info, text)
-        };
-        self.notify(request, cx);
-    }
+    /// The editor hint: a short message about what an editor command did or could not do, shown at the
+    /// caret of the active editor until the next hint replaces it. Without an editor nothing shows.
+    pub fn show_hint(&mut self, text: String, cx: &mut Context<Self>) {
+        self.show_hint_toned(text, Tone::Info, cx);
+    }
+
+    /// [`Workbench::show_hint`] in `tone`.
+    pub fn show_hint_toned(&mut self, text: String, tone: Tone, cx: &mut Context<Self>) {
+        let Some(editor) = self.editor().cloned().filter(|_| !text.is_empty()) else { return };
+        // Shown once the current effects are delivered: the caret move of the command would hide it.
+        cx.defer(move |cx| {
+            if let Some(intelligence) = EditorIntelligence::of(&editor, cx) {
+                intelligence.update(cx, |this, cx| this.show_hint_at(&text, None, tone, None, cx));
+            }
+        });
+    }
```

`app-gpui/src/editor/command_actions.rs` (webview: `src/editor/core/actions.ts:612-614` and `:435-436`):

```diff
@@ imports
+use crate::editor::intelligence::EditorIntelligence;
+use crate::editor_lsp::hint::Tone;
@@ fn next_occurrence
     if step.outcome == OccurrenceOutcome::NotFound {
-        tracing::debug!("no further occurrence of the selection");
+        show_hint(editor, "Not found, press again to search from the top", cx);
     }
@@ fn expand_word
-        ExpandOutcome::Hint(hint) => tracing::debug!("{hint}"),
+        ExpandOutcome::Hint(hint) => show_hint(editor, hint, cx),
     }
 }
+
+/// Shows `hint` (English source text) at the caret of `editor`, once the current effects are delivered.
+fn show_hint(editor: &Editor, hint: &'static str, cx: &mut Context<Editor>) {
+    let intelligence = editor.overlay().and_then(|overlay| overlay.clone().downcast::<EditorIntelligence>().ok());
+    let Some(intelligence) = intelligence else { return };
+    cx.defer(move |cx| intelligence.update(cx, |this, cx| this.show_hint_at(hint, None, Tone::Info, None, cx)));
+}
```

Tests that assert the old notification and must change with this fix: `workbench::editor_core::tests::the_copy_path_actions_copy_the_forms_of_the_edited_file` (`editor_core.rs` ~571-575: `titles(..).contains("A hint")`, `"Copied {absolute}"`), `workbench::editor_core::tests::paste_from_history_pastes_the_entry_chosen_in_the_dialog` (`"Nothing to paste: …"` in `titles`), `workbench/mod.rs` ~9983 (`notification_titles(..).contains("Copied src/notes.txt")`, the Project view's Copy Path, which goes through `editor.showHint`: with no editor open, the webview shows nothing), and `workbench::tests::proofs_editor_copy_reference` line 58 (`"Copied reference: …"`). Each should read the hint of the active editor instead (`EditorIntelligence::of(&editor, cx)` → `hint()`), or, where no editor is open, assert nothing is shown. `NotificationLevel` / `NotificationRequest` stay used elsewhere in `editor_core.rs`. Rows the fix also moves: `action:editor.copyPath`, `action:editor.copyPathWithLineNumber`, `action:editor.showHint`, `action:code.pasteFromHistory` (their "notification, not the hint" clause goes).

### 2. Background Tasks only opens the popup; the status item toggles it

Webview: `src/workbench/StatusBar.tsx:92` (`showBackgroundTasks` sets `open: true`) and `:112` (the item's click toggles).

`app-gpui/src/workbench/plugin_contributions.rs`:

```diff
     registry.handle(BACKGROUND_TASKS, |_, window, cx| {
-        on_workbench(window, cx, |workbench, window, cx| workbench.toggle_background_tasks(window, cx))
+        let toggle = call_toggles(call.arg);
+        on_workbench(window, cx, move |workbench, window, cx| {
+            if toggle || workbench.background_tasks_popup().is_none() {
+                workbench.toggle_background_tasks(window, cx);
+            }
+        })
     });
```

with the closure taking `|call, window, cx|` and, at module level:

```rust
/// The tasks status item's click asks for a toggle (`{ "toggle": true }`); the action itself only opens the popup.
fn call_toggles(arg: Option<&Value>) -> bool {
    arg.and_then(|arg| arg.get("toggle")).and_then(Value::as_bool).unwrap_or(false)
}
```

`app-gpui/src/workbench/status_items.rs:193` and its test at ~388:

```diff
-        .on_click(StatusClick::action("view.backgroundTasks"));
+        .on_click(StatusClick::Action { id: "view.backgroundTasks".to_owned(), arg: Some(json!({ "toggle": true })) });
```

and the existing `plugin_contributions.rs` test's last two lines (`run_action(cx, BACKGROUND_TASKS, None)` then `is_none()`) become `run_action(cx, BACKGROUND_TASKS, Some(json!({ "toggle": true })))`. (`Value`/`json!` imports as each file already has them; check `status_items.rs` imports `serde_json::json`.)

## Tests most likely to fail first

1. The six expected failures above (`background_tasks_…` and the five hint tests) until fixes 1 and 2 land.
2. `toggle_bookmark_with_mnemonic_gives_the_caret_line_the_typed_digit`: whether `open_popup` gives the chooser the keyboard, so its key claim sees `5`; if not, `5` is typed into the file. Also whether `alt-f3` reaches the binding (`Alt+F3` in `intellij-macos`).
3. `maximize_tool_window_draws_the_focused_side_across_the_window`: the key `cmd-shift-'` (from `Meta+Shift+'`) may be named differently by GPUI's parser; fall back to `run_action(cx, "toolwindow.maximize", None)`. `debug_bounds("hide:project")` must be the header's button (selector built with `format!("hide:{id}")`).
4. `refresh_in_the_tree_lists_a_table_created_since_it_was_read`: ⌘R in the editor is Replace there; if that moves focus into the find bar the assertion still holds. If the tables folder's refresh reloads only on a schema node, select the `main` row instead.
5. `open_in_browser_…`: the group's Open in Browser when nothing runs must be a no-op (`browser_command(&[])` is `None`); `opened_url` starts `None`.
6. `code_cleanup_in_file_leaves_out_the_steps_its_argument_turns_off`: the count of `textDocument/codeAction` requests is 1 only if nothing else (a lightbulb or intention request) asks code actions on open; if it does, filter the requests by `context.only`.
7. `ctrl_g_…`: `Editor::selections()` gives `(anchor, head)` in document order; a selection made by `select(6, 9)` is `(6, 9)`.

## Behaviours that differ from the webview, without a contained fix (B), read this session

Handler and webview source read for these:

- `action:plugins.log.wrap`: the log tab is built with soft wrap on (`plugin_runtime.rs:246`, `PluginLogTab::new(data, true, cx)`); the webview starts it at `softWrapActive(editor.softWrap ?? true, prefs, "stream.log")` (`src/features/plugins/views/logEditor.tsx:61`), i.e. off unless the soft-wrap file patterns match `stream.log`. The native editor settings have `matches_file_patterns` (`editor_model/editor_prefs.rs:32`) but nothing calls it yet; the fix is reading `editor.softWrap` and the soft-wrap files pattern there. Not written: the settings field names need checking.
- `action:editor.pasteHistory.paste`, `action:editor.pasteHistory.remove`: `pasteHistoryListFocus` is never set (`mod.rs` sets only `pasteHistoryOpen`); the native dialog has one focus handle and a field, no list focus, so Enter/Delete are the dialog's own claimed keys. A fix needs a list focus in `editor/paste_history_view.rs`.
- `action:vcs.showLog`: a `{ branch }` or `{ path }` argument should open a log tab of its own (`src/features/vcs/actions.tsx:512`); native `show_log` (`vcs_operations.rs:974`) logs and shows the main log. `vcs_ops::log_store::GitLog::open_log_tab` exists, but the Git window's view draws no log tabs.

## Ids skipped, with why

- Covered by a test in the proofs3 draft (implement/override/quick definition/go to test/rename/previous highlighted usage by their keys), or left out there with a reason this assignment also excludes (Docker daemon, system Trash, full screen, Java language server for hot swap, the fake server's missing error answer for the usages fallback): `app.quit`, `code.implementMethods`, `code.overrideMethods`, `code.quickDefinition`, `debug.cancelHotSwap`, `debug.hotReload`, `debug.hotRestart`, `debug.reloadChangedClasses`, `debug.toggleFieldWatchpoint`, `debug.toggleMethodBreakpoint`, `docker.refresh`, `docker.toggleStoppedContainers`, `docker.toggleUntaggedImages`, `file.openFile`, `file.openInNewWindow`, `find.usages`, `navigate.gotoTest`, `navigate.prevHighlightedUsage`, `project.delete`, `project.rename`, `refactor.rename`, `run.reloadConfigs`, `run.runConfig`, `run.stopProcess`, `services.addService`, `window.toggleFullScreen` (26).
- `merge.annotate`: in the `proofs_merge_*` drafts being compiled.
- `navigate.gotoBookmark0`…`9`, `navigate.toggleBookmark0`…`9` (20): the dynamic row `action:(dynamic) src/features/search/index.ts:476` / `:486` is done.
- `toolwindow.bookmarks`, `toolwindow.commit`, `toolwindow.debug`, `toolwindow.find`, `toolwindow.git`, `toolwindow.problems`, `toolwindow.project`, `toolwindow.run`, `toolwindow.services`, `toolwindow.structure`, `toolwindow.terminal` (11): the dynamic row `action:(dynamic) src/workbench/toolwindows/actions.ts:160` is done; `toolwindow.resizeDown`, `toolwindow.resizeLeft`, `toolwindow.resizeRight`, `toolwindow.resizeUp` (4): `action:(dynamic) src/workbench/toolwindows/actions.ts:247` is done.

## Ids not reached

Classified only from the gap their row records (`registered_not_done.md`); the handler, the cited test and the webview source were NOT read for these, so each class is a hypothesis to check. Areas with no test from this pass: debugger, agents, version control, search popups, file and project, build, terminal, most of the editor's commands.

### Recorded gap reads as a behaviour that differs (likely B)

- `action:vcs.showCommit`, `action:vcs.log.goToRef`: a commit not yet loaded is not paged in.
- `action:vcs.log.pushUpToHere`: the Push dialog lists every outgoing commit, not those up to the chosen one.
- `action:vcs.manageRemotes`: a password inside a remote URL is masked; the webview shows the URL as stored (a decision, not a bug fix).
- `action:vcs.newTag`: one-line message field where the webview has a multi-line one.
- `action:vcs.shelve`, `action:vcs.showShelf`: shelf rows show no time.
- `action:terminal.closeSession`: no question while a command runs.
- `action:editor.closeTab`: `files.autoSave` is not read (D23).
- `action:editor.unsplit`, `action:editor.unsplitAll`: no single question about the unsaved tabs of the closed pane.
- `action:file.reloadAllFromDisk`: `project.refresh` is not registered, so the Project view is not read again.
- `action:navigate.nextHighlightedUsage`: its fallback `navigate.nextOccurrence` is not registered.
- `action:navigate.back`, `action:navigate.forward`: places inside one file are not recorded.
- `action:db.execute`: with two or more sources a plain .sql file only says a source must be chosen (no chooser).
- `action:find.openInWindow`: the bar's In Selection scope, excluded occurrences and unsaved text are not carried over.
- `action:find.showUsages`: the popup opens in the middle of the window, not at the caret.
- `action:find.highlightUsages`: the status text is kept but the status bar does not draw it.
- `action:code.parameterInfo`: Tab / Shift+Tab between arguments are not wired.
- `action:editor.pinTab`: pinned tabs are not drawn differently.
- `action:edit.copy`, `action:edit.cut`, `action:edit.paste`, `action:edit.redo`, `action:edit.selectAll`, `action:edit.undo`: act on code editors only, not text fields and lists.
- `action:editor.increaseFontSize`, `action:editor.decreaseFontSize`, `action:editor.resetFontSize`: the focused font zoom surface (log, plugin content view) is not stepped first.
- `action:code.applyQuickFix`, `action:code.generate`: the inserted lines are not reformatted.
- `action:code.reformat`, `action:code.reformatFile`: a plugin-contributed formatter is not used.
- `action:navigate.declaration`: without a server only names local to the file are found (the UI's syntax fallback).
- `action:find.inPath`, `action:find.replaceInPath`: the preview, row colours, resizing, scope editor and the rest listed in their rows.
- `action:debug.pluginStart`: besides the JDWP branch (Java, out of scope), a waiting request is answered when the session ends rather than when it starts, which is a (B) a fake adapter could test.

### Recorded gap reads as a view not built (likely C)

- `action:agents.dockTask`, `action:agents.openTask`, `action:agents.openTaskInEditor`, `action:agents.undockTask`, `action:agents.useEditor`, `action:agents.useToolWindow`: a conversation cannot be an editor tab (not built).
- `action:build.buildArtifacts`, `action:build.buildModule`, `action:build.buildProject`, `action:build.rebuildProject`: a caller cannot wait for the build's end (no completion handle in the native build runner).
- `action:code.collapseBlock`, `action:code.collapseRecursively`, `action:code.expandRecursively`, `action:code.fold`, `action:code.foldAll`, `action:code.foldSelection`, `action:code.foldToggle`, `action:code.unfold`, `action:code.unfoldAll`: the gutter draws no open-region chevrons and Code Folding's fold-by-default is not applied on open (view not built).
- `action:code.commentBlock`, `action:code.commentLine`, `action:code.completeStatement`, `action:code.moveElementLeft`, `action:code.moveElementRight`, `action:code.moveStatementDown`, `action:code.moveStatementUp`, `action:code.toggleCase`, `action:code.extendSelection`, `action:code.shrinkSelection`: need the document's syntax tree in the editor (or the backend's structural answers), which the editor does not have.
- `action:code.completion`: snippet fields, live templates, contributed rows, word completion without a server.
- `action:code.quickDocumentation`: the Documentation popup with history and tool window is not built.
- `action:code.showIntentions`: the bulb and the IDE's own intentions are not wired.
- `action:db.editData`: the table opens in the Database window, not an editor tab.
- `action:db.explainPlan`: the Query Plan tab is not built.
- `action:db.openConsole`: the console toolbar is not built.
- `action:editor.copyPathReference`: Path From Source Root needs the project's source roots, which nothing computes.
- `action:editor.toggleColumnSelection`: dragging a block with the mouse is not built.
- `action:file.openProject`: the system folder dialog, not the webview's own Open dialog with recent locations.
- `action:file.openProjectPath`, `action:file.openRecentProject`: Attach and remote projects are not built.
- `action:file.openRecent`, `action:welcome.recentProjects`: the Recent Projects popup is not built.
- `action:file.settings`, `action:settings.appearance`, `action:settings.keymap`: pages and controls not built.
- `action:file.toggleReadOnly`: virtual documents are not modelled.
- `action:navigate.fileStructure`, `action:navigate.findAction`, `action:navigate.gotoFile`, `action:navigate.gotoSymbol`, `action:navigate.searchEverywhere`, `action:navigate.searchText`: the gaps are the popup's own (`search.everywhere`, `search.fileStructure` overlays).
- `action:navigate.gotoLine`: a popup where the webview has a dialog.
- `action:navigate.implementation`: implementations are not streamed.
- `action:navigate.recentFiles`, `action:navigate.switcher`: Recent Locations and the files edited in this session are not built.
- `action:navigate.showBookmarks`: the preview is plain text, not a second editor.
- `action:plugins.installFromDisk`: the picker without an argument (a system file dialog) and package files; not reached for a test (see below).
- `action:plugins.settings`: the plugin listing inside Settings needs a running plugin host in the settings test.
- `action:project.copy`, `action:project.cut`, `action:project.paste`: files are not put on the system clipboard.
- `action:run.editConfigs`: templates, import, run targets, environment table, Tomcat settings.
- `action:run.run`: Current File hands over to `run.runContext`, which is not built.
- `action:run.stop`: contributed debug sessions are not counted (the debugger is not drawn).
- `action:vcs.branches`, `action:vcs.newBranch`, `action:vcs.push`: several repositories, Compare / Show Diff with Working Tree, the tag menu, rejected-push retry.
- `action:vcs.log.changes.compareBeforeWithLocal`, `action:vcs.log.changes.compareWithLocal`, `action:vcs.log.compareVersions`, `action:vcs.log.compareWithLocal`, `action:vcs.shelf.showDiffNewTab`, `action:vcs.showDiff`: the diff is a front window, not an editor tab (and no "Changes between" list).
- `action:vcs.log.collapseLinear`, `action:vcs.log.expandLinear`: the collapsed graph is not drawn.
- `action:vcs.log.squashCommits`: the log has no multi-selection.
- `action:vcs.resolveConflicts`: the merge editor is a front window, no blame or syntax colours.

### Gap text suggests a test is possible (A) but not attempted

- `action:code.extendSelection`, `action:code.shrinkSelection`: the gap is that no step is checked against a range only the backend's syntax tree gives; listed above as needing a syntax tree, which contradicts the cited test `editor::core_actions::tests::extend_and_shrink_selection_walk_the_syntax_ranges_around_the_caret`. Read `editor/core_actions.rs` before classing them.
- `action:file.reloadAllFromDisk`: `vcs.refresh` runs and no test asserts it (A); the unregistered `project.refresh` is (B).
- `action:plugins.installFromDisk`, `action:plugins.settings`: `plugins_window::test_support` has plugin fixtures and may reach both.

## Count (of the 198)

- With a test here: 16 ids (`bookmarks.toggleFileBookmark`, `navigate.toggleBookmarkWithMnemonic`, `db.refresh`, `run.services.openBrowser`, `run.services.hideConfig`, `toolwindow.maximize`, `view.backgroundTasks`, `code.cleanupFile`, `code.quickTypeDefinition`, `editor.showHint`, `editor.copyPath`, `editor.copyPathWithLineNumber`, `code.pasteFromHistory`, `code.addCaretNextOccurrence`, `code.cyclicExpandWord`, `code.cyclicExpandWordBackward`) in 13 tests. Done at once: 5; done after fixes 1 and 2: 6 more (11 in all); partial: 5.
- (B) with a fix (hunks above), all among the tested ids: 8 (`view.backgroundTasks`, `editor.showHint`, `editor.copyPath`, `editor.copyPathWithLineNumber`, `code.pasteFromHistory`, `code.addCaretNextOccurrence`, `code.cyclicExpandWord`, `code.cyclicExpandWordBackward`).
- (B) read this session, no contained fix: 4 (`plugins.log.wrap`, `editor.pasteHistory.paste`, `editor.pasteHistory.remove`, `vcs.showLog`).
- (C) confirmed by reading: 0.
- Skipped: 62.
- Not reached (classified from the recorded gap only, unread): 116.
- 16 + 4 + 62 + 116 = 198.
