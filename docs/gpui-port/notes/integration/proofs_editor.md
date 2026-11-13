# Proof tests: the EDITOR groups of needs_test.md

None of these tests has been compiled or run. No existing file was edited.

This one file holds every `mod` line, row, hunk and risk for all four groups. Parts B, C and D below were written by helper agents. I checked those parts for the following:

- every method name they call exists (`fn <name>` in app-gpui/crates, or a std/GPUI API);
- the shared highlight helpers are `pub(super)`;
- the safe-delete fixture waits for `didOpen` before acting.

Their test logic was not re-derived line by line.

## All `mod` lines

| File                                               | Anchor (inside its `mod tests {`)                                  | Lines to add                                                                                                                                                                                                                                                                                                                                                                    |
| -------------------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/editor/intelligence.rs`              | after `    use std::path::PathBuf;`                                | `mod proofs_editor_lsp;` `mod proofs_editor_navigation_modifier;`                                                                                                                                                                                                                                                                                                               |
| `app-gpui/src/workbench/mod.rs`                    | after the first `    use std::time::Duration;` under `mod tests {` | `mod proofs_editor_actions_find;` `mod proofs_editor_actions_markdown;` `mod proofs_editor_actions_navigate;` `mod proofs_editor_actions_run_console;` `mod proofs_editor_actions_tabs;` `mod proofs_editor_copy_reference;` `mod proofs_editor_search_everywhere;` `mod proofs_editor_search_find;` `mod proofs_editor_search_recent;` `mod proofs_editor_search_safe_delete;` |
| `app-gpui/src/editor/view.rs`, `mod tests`         | see part D                                                         | `mod proofs_editor_highlight_backend;` `mod proofs_editor_highlight_cmake;` `mod proofs_editor_highlight_interpolation;` `mod proofs_editor_highlight_shell;` `mod proofs_editor_highlight_support;`                                                                                                                                                                            |
| `app-gpui/src/editor/view.rs`, `mod editing_tests` | see part D                                                         | `mod proofs_editor_clipboard_history;` `mod proofs_editor_multi_caret;`                                                                                                                                                                                                                                                                                                         |
| `app-gpui/src/find/popup_view.rs`                  | after `    use std::cell::RefCell;`                                | `mod proofs_editor_project_masks;`                                                                                                                                                                                                                                                                                                                                              |

Each `mod` goes on its own line, four spaces in.

## IPC rows to decide (strict reading of the brief)

Native `ipc:doc_close` and `ipc:doc_set_encoding` (part B) call `documents.close` / `documents.set_encoding` directly. They do not go through `ide_host::ops::doc::close` / `doc_set_encoding`. Those ops are one-line pass-throughs to the same functions (`ops/doc.rs:19-22`), so done is defensible. Under the brief's literal rule ("calls the same `ide_host::ops::…` function"), mark them partial with that reason. `ipc:languages` (part D) is in the same position: `host.languages.all()` is read directly.

# Part A — editor::intelligence (33 rows)

Tests are in `app-gpui/src/editor/intelligence/tests/proofs_editor_lsp.rs` (7 tests).

## mod line

In `app-gpui/src/editor/intelligence.rs`, inside `mod tests {` (about line 1608), after the anchor `    use std::path::PathBuf;` (unique in that module):

```rust
    mod proofs_editor_lsp;
```

The misc agent's `    mod proofs_editor_navigation_modifier;` goes on the same anchor.

## Server attach: what every test waits for

`open()` returns once the editor has the keyboard. It does not wait for the server to have the document, so whether the first key reaches the server is a race.

Hypothesis, not measured: before the server is ready, `lsp_completion` answers `Unavailable`, `CompletionSource::answer` starts its back-off, and that would cause the intermittent `typing_completes_from_the_server_…` count. Against it, `ready_targets` awaits readiness. To check: what `LspManager::ready_targets` (crates/ide-lsp/src/lib.rs) returns when no server has started yet for the document's language, and whether the `DIAG all=` line of a failing run shows `didOpen` after the first `textDocument/completion`.

The fixture in this file, `open_with`, settles until the fake server's log holds a `textDocument/didOpen` for the opened file's uri. It returns only then. The server writes that entry only after `initialize` and once the backend has attached the document. That is the signal to wait on. The same one-line wait fixes the existing test: `settle(cx, |_| !f.requests("textDocument/didOpen").is_empty());` right after `open(…)`.

## The fixture extends the fake server without editing it

`ide-lsp` checks capabilities before sending. `doc_targets(… c.has("referencesProvider") / "callHierarchyProvider" / "typeHierarchyProvider")` does this, and `execute_command` looks at `executeCommandProvider.commands`. The fake server's `initialize` offers none of these, so without them the requests never reach it. Call Hierarchy would then fall back to text matching in silence.

`install_with_capabilities` therefore calls `FakeServer::install`, then rewrites the `server.py` it wrote. After the `'codeActionProvider': {'resolveProvider': True},` entry it adds `**script().get('capabilities', {}),`. Each test then names extra capabilities under `"capabilities"` in its answers. The answers are read again at `initialize`, including after a restart. It asserts that the anchor exists, so a change to the script fails loudly rather than silently.

If the integrator prefers it, the same one-line change can go into `SERVER` in `editor/intelligence.rs` (hunk below). `install_with_capabilities` then becomes plain `FakeServer::install`.

```diff
             'codeActionProvider': {'resolveProvider': True},
+            **script().get('capabilities', {}),
         }}
```

## Tests and the rows they prove

1. `editor::intelligence::tests::proofs_editor_lsp::an_accepted_completion_items_command_runs_on_the_server`
   - `ipc:lsp_execute_command` → **done**. After Enter accepts an item that carries `command`, the server receives exactly one `workspace/executeCommand` with that command and arguments, and the text holds the item. The native path (`accept` → `FollowUp::ExecuteCommand` → `ops::lsp::lsp_execute_command`) and `src-tauri/src/commands/lsp.rs:434` call the same op.
2. `…::call_hierarchy_lists_the_callers_the_server_reports`
   - `action:navigate.callHierarchy` → **done**. The upgrade fills its gap, "no test has a hierarchy to show": the action runs through the registry, the Hierarchy tool window is titled "Callers of target", and the root's children are the caller the server reported (name and file).
   - `ipc:lsp_call_hierarchy` → **done**. The server received `textDocument/prepareCallHierarchy` and `callHierarchy/incomingCalls`, and the row comes only from that answer. The `hierarchy_level` → `ops::lsp::lsp_call_hierarchy` path matches the adapter at `commands/lsp.rs:361`.
3. `…::type_hierarchy_lists_the_subtypes_the_server_reports`
   - `action:navigate.typeHierarchy` → **done**. The window is titled "Subtypes of Base" and lists Derived.
   - `ipc:lsp_type_hierarchy` → **done**. The server received `prepareTypeHierarchy` and `typeHierarchy/subtypes`, and no `supertypes`. The default direction of a type hierarchy is Subtypes, the same in `src/features/code-insight/hierarchy.ts:238`. The op matches the adapter at `commands/lsp.rs:373`.
4. `…::find_usages_lists_the_references_the_server_reports`
   - `ipc:lsp_references` → **done**. It upgrades the gap "no language server answers in tests". The Find Usages rows are exactly the server's two locations. The name's other three text matches (the declaration, a second use, a comment) are left out, so the rows cannot be text search. The params carry the caret position. `fetch_references` → `ops::lsp::lsp_references` matches `commands/lsp.rs:158`.
5. `…::the_server_status_item_lists_the_projects_server_and_restart_starts_it_again`
   - `ipc:lsp_servers` → **done**. The status item's `mock-ls` entry carries `languages == ["mock"]` and `root == project root`. A `LanguageServer` event adds an entry with no languages and no root, so only `Servers::snapshot` of `ops::lsp::lsp_servers` fills them (`code_insight/servers.rs:192` vs `:245`). The adapter is at `commands/lsp.rs:379`.
   - `ipc:lsp_restart` → **done**. The test emits `ServerStatusEvent::Restart(id)` from the item, which is what its Restart row does. The server log then shows a second `initialize`, and the entry comes back Running. The adapter is at `commands/lsp.rs:392`.
6. `…::a_context_action_that_edits_two_files_changes_both`
   - `module:src/editor/lsp/workspaceEdit.ts` → **partial**. The intention preview's diff and its "Also changes 1 other file" note match `src/editor/lsp/intentions.ts:366-374`. Enter applies the edit to the open file and to the unopened one (read through `documents.text_of`, else from disk). Missing: `summarizeWorkspaceEdit` / `describeSummary` / `needsPreview` ("3 occurrences in 2 files", which the rename preview uses). This test does not reach them.
   - It also adds to `module:src/editor/lsp/intentions.ts` (partial) and `ipc:lsp_apply_code_action` (already done) for multi-file edits.
7. `…::a_quick_fixs_arrow_menu_opens_with_the_right_arrow_and_runs_its_controls`
   - `unit:src/editor/lsp/Popups.test.tsx` → **partial**. It covers the spec case "opens a row's → menu with the right arrow and runs its items": → opens the controls with the first selected, ↓ moves, ← closes, and choosing Disable inspection turns the inspection off without applying the fix. The spec has 13 cases. This test, plus the existing `show_intentions_lists_the_code_actions_and_applies_the_chosen_one` (lazy preview) and `cmd_click_goes_to_the_declaration_or_chooses_between_several` (selection reported, Enter opens), covers 3 of them.
   - Missing:
     - "numbers rows 1-9": no native popup sets `ListOptions::numbered`, and nothing uses Refactor This.
     - "pins the results elsewhere": the chooser's pin is not driven here.
     - The imports-on-paste dialog (2 cases), documentation history and tool window (4), quick definition (1) and the reformat dialog (1) are other components.
   - `module:src/editor/lsp/Popups.tsx` and `module:src/editor/lsp/popupStore.ts` → **partial**, citing this test plus the two above.
   - **Expected to fail** at `settle(cx, |cx| inspections(cx) != before)`. Natively the `Control` event only closes the popup: `workbench/editor_intelligence.rs:107` and `editor/intelligence.rs` both drop it, and nothing runs the control. The webview runs the control item (`Popups.test.tsx:87-116`, `disable` called once). The fix is below.

### Fix that test 7 depends on

`app-gpui/src/workbench/code_insight.rs`, make the handler reachable from the sibling module:

```diff
-    fn apply_inspection(&mut self, command: InspectionCommand, window: &mut Window, cx: &mut Context<Self>) {
+    pub(super) fn apply_inspection(&mut self, command: InspectionCommand, window: &mut Window, cx: &mut Context<Self>) {
```

`app-gpui/src/workbench/editor_intelligence.rs`, in `show_intentions`:

```diff
-                ListPopupEvent::Dismissed | ListPopupEvent::Pin | ListPopupEvent::Control { .. } => {
+                ListPopupEvent::Control { control, .. } => {
+                    workbench.close_popup(window, cx);
+                    let _ = intelligence.update(cx, |intelligence, _| intelligence.intentions_closed());
+                    if let Some(run) = control.run.clone() {
+                        workbench.apply_inspection(run, window, cx);
+                    }
+                }
+                ListPopupEvent::Dismissed | ListPopupEvent::Pin => {
                     workbench.close_popup(window, cx);
                     let _ = intelligence.update(cx, |intelligence, _| intelligence.intentions_closed());
                 }
```

`InspectionCommand` is already what `ControlItem::run` holds (`platform_more/inspection_controls.rs`). Even after this fix, the editor's own underline does not follow a disabled inspection, because `EditorIntelligence` styles with `ProfileOverrides::default()` (intelligence.rs `diagnostics_published`). The test therefore asserts the stored profile, not the underline.

## Rows of the group that stay partial by citation (no new test)

Each of these can be `[~]` now, citing the existing five `editor::intelligence::tests::*` named in needs_test.md plus tests 1, 6 and 7 above:

- `module:src/editor/lsp/completion.ts`, `completionItems.ts`, `autoImport.ts`, `snippet.ts`
- `module:src/editor/lsp/hover.ts`, `docs.ts`, `markdown.ts`, `markdownParser.ts`
- `module:src/editor/lsp/diagnostics.ts`, `signature.ts`, `intentions.ts`, `navigation.ts`
- `module:src/editor/navigation/index.ts`

`module:src/editor/lsp/index.ts`, `actions.ts` and `theme.ts` follow the others, as needs_test.md says.

## Recipes not written, and why

- **(a) Go to declaration where the server answers nothing, so the syntax fallback runs** (syntaxFallback, files, fileReferences).
  - `GotoFlow::answered` takes the fallback only on an error; an empty answer gives the "Cannot find declaration" hint, which is already tested.
  - The fake server never sends an error, and `mock` has no grammar, so `syntax_locals` would fail anyway.
  - The fallback itself is proven by `workbench::tests::go_to_declaration_without_a_server_finds_a_local_through_the_syntax_tree` (Light mode, no server).
  - `files.ts` (chooser names) is exercised by `cmd_click_…` (labels `b.mock · b.mock:3`).
  - `fileReferences.ts` is path completion inside `href`/`src`/imports, which is unrelated to navigation and needs its own completion test.
  - `unit:src/editor/lsp/syntaxFallback.test.ts` was not mapped.
- **(c) Unavailable hint and back-off** (hint, lsp, availability).
  - Without the capability, `ide-lsp` answers `Unavailable` before sending, so "a repeat does not reach the server" holds whether or not there is a back-off, and proves nothing.
  - A real proof needs the server to answer an error the backend maps to `Unavailable`. The fake server only ever sends `result`. The fix is to teach it `{"error": …}` answers: in `SERVER`, `if isinstance(result, dict) and 'error' in result: send({'jsonrpc': '2.0', 'id': msg['id'], 'error': result['error']}); continue`. Then check `rpc_err`'s mapping of the code to `ErrorKind::Unavailable`.
- **(d) ⌘-click on a key in application.properties or YAML** (configurationNavigation). The fake server serves only `*.mock`. Pointing it at `.properties` means overriding the built-in properties language from the config `languages/` folder, and I did not verify that the override wins. The native module is reached only for those languages.
- **(e) numbered rows**: not reachable (see test 7).
- **Structure popup inherited members** (`ipc:doc_open_virtual`). Not written. It needs `documentSymbol`, `prepareTypeHierarchy` and `typeHierarchy/supertypes` answers plus a virtual-uri document the fake server would also have to serve (`workspace/textDocumentContent`). With a second `.mock` file instead, the row would not be proven, so it stays open.
- **`point.registerEditorContribution:lsp`**: notes only. This is in the actions sub-notes.

## Most likely to fail on first run

1. Test 7, at the inspection-profile assertion, by design (fix above). Before that point, the → menu holds the two controls only if `diagnostic_inspection` gives an inspection for producer `mock-ls`/`mock` and code `MOCK001`, and the fix row is `is_fix()`. Both were checked by reading only.
2. Test 5, on `server.languages == ["mock"]` or `root`. The snapshot's root is the server's workspace root, `to_ipc(&info.root)`. For a language with no root markers I assumed that is the project root.
3. Test 4, if `references_streaming` widens the search or merges the indexed symbol and adds rows. The assertion would then show the extra rows.
4. Test 1, if a single item is auto-inserted: rows would be empty before Enter. `auto_insert_single` applies to explicit completion only, as far as I read.
5. Any test, if the `server.py` rewrite fails. The anchor assert then names it.

# Part B — Editor actions and find (without its context keys), run console key claim

## proofs_editor_actions — notes

## mod lines

All files are children of the inline `mod tests` of `app-gpui/src/workbench/mod.rs`. Add, inside that module, right after its anchor line `    use std::time::Duration;` (the fourth line of `mod tests {`, after `use super::*;`, `use crate::backend::AppDirs;`, `use gpui_kit::{TestAppContext, VisualTestContext};`):

```rust
    mod proofs_editor_actions_find;
    mod proofs_editor_actions_markdown;
    mod proofs_editor_actions_navigate;
    mod proofs_editor_actions_run_console;
    mod proofs_editor_actions_tabs;
```

## app-gpui/src/workbench/tests/proofs_editor_actions_find.rs

- `the_find_in_files_popups_commands_move_the_selection_list_recent_searches_and_open_the_match` → `action:find.popup.nextResult`, `action:find.popup.previousResult`, `action:find.popup.openResult`, `action:find.popup.recentSearches` — each id runs through `run_action` with the popup open on "widget": Next/Previous Result move the selected row and wrap at both ends (`FindPopup::selected`), Recent Searches opens the Find history list with the seeded "old query" (`menu()`/`menu_lines()`), Open Result opens `src/alpha.rs` with the caret on the last match (3,4) and closes the popup. All four → done. Matches the webview (`FindInFiles.tsx` command handler: `(active ± 1 + n) % n`, `openRow(selected)` closes an unpinned popup, recent menu picks find/replace by focused box).
- `scroll_to_results_during_typing_turns_the_selection_while_typing_off_and_on` → `action:find.scrollToResults` — with the bar open the toggle reads `Some(true)`, runs to `Some(false)`, typing "widget" then counts 4 but leaves the editor selection at 0..0 while Enter still selects 11..17; run again it reads `Some(true)` and a new query selects the nearest occurrence (27..31). Done. The webview keeps this preference in memory only (`useFindPreferences`, zustand without persist), so no stored value is asserted (the sweep's "stored preference follows" does not apply).

## app-gpui/src/workbench/tests/proofs_editor_actions_navigate.rs

`mod` line: `    mod proofs_editor_actions_navigate;` (same anchor).

- `go_to_class_finds_a_type_by_name_and_opens_it` → `action:navigate.gotoClass` — the action opens Search Everywhere on `SeTab::Classes` with the query focused; "Widg" lists one row, the struct `Widget` and not `make_widget` (types only: `IndexFilter::Types`), and Enter opens `src/alpha.rs` at (0,11). Done.
- `previous_bookmark_goes_back_through_the_bookmarks_and_round_to_the_last` → `action:navigate.prevBookmark` — three bookmarks (alpha:0, alpha:2, beta:0) set through `navigate.toggleBookmark`; from the caret on alpha:0, four runs go beta:0 (wrap), alpha:2, alpha:0, beta:0 (wrap again), caret at column 0 each time. Done. Same order and wrap as the webview's `jumpBookmark(-1)` (sorted by path then line, `reverse().find(k < cur) ?? last`, col 0).
- `the_switcher_opened_backward_starts_on_the_oldest_editor_and_opens_it_when_ctrl_is_released` → `action:navigate.switcherBack` — with Ctrl held and no popup, the action opens the Switcher (`is_switcher`) listing notes/beta/alpha with `src/alpha.rs` (the oldest, last file row) selected; releasing Ctrl opens it and closes the popup. Done (the gap was "opening it backward is tested in the view only"). The webview's `cycleSwitcher(-1)` opens with `step: -1` from `baseIndex = files.length`, i.e. the last file row, which this asserts.

## app-gpui/src/workbench/tests/proofs_editor_actions_tabs.rs

`mod` line: `    mod proofs_editor_actions_tabs;` (same anchor).

- `close_all_tabs_closes_the_pinned_tab_with_the_others` → `action:editor.closeAllTabs` (with the next test) — a pinned `alpha.rs` (pinned via `editor.pinTab`, read from `TabsState`) and `notes.txt` both close: no tab titles, no editor, `editorTabsOpen` off. Should pass as is (`close_all(pane, false)`).
- `close_all_tabs_closes_every_pane_and_leaves_nothing_to_reopen` → `action:editor.closeAllTabs` — with the editor split (left: alpha, notes; right: notes), Close All Tabs leaves every pane empty and `editor.reopenClosedTab` then brings nothing back. **Expected to fail** (see the fix hunk below): native runs `close_all` on the active pane only and keeps `TabsState::closed`. The webview's action calls `ctrl.closeEverything()` (`src/editor/core/controller.ts`): `closeAll(next, id)` for every pane id, `keepPinned` false, with `{ clearHistory: true }` (`closed: []`). Row becomes done with both tests passing; until the fix lands it stays partial.
- `closing_a_tab_releases_its_document_once_no_other_tab_shows_it` → `ipc:doc_close` — the backend's `open_documents()` (mapped to file names through `documents.identity`) goes alpha+notes → (split: still both, one document for two panes) → (close the split's tab: still both) → alpha only → empty. Done. Note on the IPC claim: the adapter `src-tauri/src/commands/doc.rs::doc_close` calls `ide_host::ops::doc::close`, which is `host.project(window)?.documents.close(doc)`; the native editor calls `documents.close(editor.doc)` directly in its `on_release` (`app-gpui/src/editor/view.rs` ~line 310). Same backend function, not the `ops` wrapper. The existing `files_open_in_tabs_that_the_keymap_cycles_closes_and_reopens` already asserts `open_documents().is_empty()` at its end, so the parity.json gap text was stale; this test adds the per-tab and shared-document cases.
- `reload_with_encoding_reads_the_file_again_in_the_chosen_encoding` → `ipc:doc_set_encoding` — `menu.txt` written as UTF-8 "café\n" opens as UTF-8; the encoding item's menu offers Reload (not disabled), Right opens it, the keys choose `windows-1252`, and the editor then reads "cafÃ©\n" (bytes re-read from disk and decoded so; only a reload can produce it), the item reads "windows-1252", the editor is not dirty and the file is unchanged. Done (the gap was "reloading only as the refusal"). IPC check: adapter `doc_set_encoding` → `ide_host::ops::doc::doc_set_encoding` (`crates/ide-host/src/ops/doc/commands.rs`) → `project.documents.set_encoding(doc, &encoding, reload)`; native `Editor::set_encoding` (`editor/view.rs` ~958) → `self.documents()?.set_encoding(self.doc, encoding, reload)`. Same backend function, not the `ops` wrapper. windows-1252 is chosen rather than ISO-8859-1 because `encoding_rs` maps the label `iso-8859-1` to windows-1252, so the item would read "windows-1252" after choosing "ISO-8859-1" (same in the webview, which shows what the backend returns).

### Fix hunk the second Close All Tabs test needs (`app-gpui/src/workbench/mod.rs`, in `register_actions`)

```rust
-    registry
-        .handle("editor.closeAllTabs", |_, window, cx| on_tabs(window, cx, |tabs, pane| tabs.close_all(pane, false)));
+    registry.handle("editor.closeAllTabs", |_, window, cx| {
+        on_tabs(window, cx, |tabs, _| {
+            let panes: Vec<String> = tabs.pane_ids().into_iter().map(str::to_owned).collect();
+            let mut changed = false;
+            for pane in &panes {
+                changed |= tabs.close_all(pane, false);
+            }
+            // Close All Tabs is not undone tab by tab: Reopen Closed Tab has nothing after it.
+            tabs.closed.clear();
+            changed
+        })
+    });
```

(`close_all` on a pane already collapsed by an earlier `remove_pane` returns false, so the loop is safe; `with_tabs` then runs `tabs_changed`, which releases every closed editor. Clearing `closed` before `tabs_changed` is safe: a tab whose unsaved text cannot be written comes back through `restore_unsaved`, which calls `tabs.open_tab`, not `closed`/`reopen_closed`.)

## app-gpui/src/workbench/tests/proofs_editor_actions_run_console.rs

`mod` line: `    mod proofs_editor_actions_run_console;` (same anchor).

- `the_run_consoles_find_bar_keeps_its_keys_from_the_ide_keymap` → `point.registerKeyClaim:(at) src/features/run/ConsoleView.tsx:346` — in a workbench with the default keymap, a project run of `/usr/bin/printf` prints three "hello" lines; with an editor open on `notes.txt` and the console's bar open (⌘F), F3 and ⌘G each move the bar's counter to the next match, F3 adds no bookmark (default macOS keymap: F3 = `navigate.toggleBookmark`, `when: projectOpen`), ⌘E opens no Recent Files popup, and Escape closes the bar with the keyboard back in the console, not in the editor (keymap: Escape = `toolwindow.focusEditor`, `when: toolWindowFocus && !popupOpen`). **Expected to fail on the ⌘E assertion**, so the row stays partial until the fix: the webview claim (`registerKeyClaim((e) => !!e.target.closest("[data-console-find]"))`, checked by `isClaimed` in `src/platform/keymap.ts` before the keymap) keeps the whole keymap out while the bar has focus, whatever the key. Natively `find_key` (`run/view.rs` ~797) returns `false` for keys it does not handle, and the interceptor in `platform/input.rs` (`init`, ~97-111) then runs `keystroke(event…)`, the keymap: ⌘E (`navigate.recentFiles`, `when: projectOpen`) opens Recent Files. Returning `true` from `find_key` for every key is not the fix (it would also stop the text field's own typing, ⌘A/⌘V, arrows). Fix sketch: a second kind of claim in `platform/input.rs`, e.g. `pub fn keep_keymap_out(focus: FocusHandle, cx: &mut App) -> u64` stored in `KeyClaims` as focus-only entries; in the interceptor, when no claim consumed the key and such a focus is focused, skip `keystroke(event, window, cx)` and do not stop propagation (the key still reaches the focused field); `RunView::new` registers the find field's focus with it (released with the other claims in `on_release`). Without the ⌘E assertion the other four keys pass and would only prove the bar wins for the keys it handles. ⌘G's keymap binding (`find.next`, `when` EDITOR_WHEN) is not live with a tool window focused, so ⌘G only shows the bar handles it. If the integrator's platform keymap is not the macOS IntelliJ one, F3/⌘E/Escape may have other bindings.
- Diff window key claim (`point.registerKeyClaim:(at) src/features/vcs/diffDialog.tsx:85`): no new test. Stays partial citing `vcs_diff::dialog_view::tests::keeps_the_changes_tree_mounted_underneath_and_closes_on_esc` (Escape; test name checked, at `vcs_diff/dialog_view.rs` ~393, `input::claim_keys` at ~118). Missing, per the sweep (the ⌘W side not re-checked in the webview source): the webview claims the `toolwindow.closeContent` bindings (⌘W) for the diff window, natively only plain Escape is claimed.

## app-gpui/src/workbench/tests/proofs_editor_actions_markdown.rs

`mod` line: `    mod proofs_editor_actions_markdown;` (same anchor).

- `a_markdown_preview_follows_the_editors_scrolling_and_the_hidden_editors_caret` → `point.registerEditorContribution:viewers.markdownLive` — a 2000-line `NOTES.md` (paragraph n on source line 2n): in Editor and Preview, `Editor::scroll_to_line(40)` scrolls the preview to the top of the block of line 40 (`MarkdownView::scroll_top` vs `layout()`); then in Preview Only, the hidden editor's caret selected at line 1600 scrolls the preview to the block of line 1600. Stays **partial**: the font zoom the webview publishes (`live.ts`: `publish(path, { fontZoom })`) is not published natively (`tab_views.rs` ~728 `live.opened(&path, &text, 0.0)`, checked). Text publication is already proven by `a_markdown_file_opens_as_text_with_its_layouts_and_a_preview_that_follows_the_typing`. The webview contribution publishes the same things this covers (`topLine/scrollSeq` on scroll, `caretLine/caretSeq` on selectionSet without docChanged, text).

## Rows with a notes entry only (no new test)

- `point.registerEditorContribution:lsp` → partial, citing (names checked) `editor::intelligence::tests::typing_completes_from_the_server_and_enter_inserts_the_item_with_its_import`, `editor::intelligence::tests::a_word_typed_in_one_go_asks_the_attached_server_once_at_its_end`, `editor::intelligence::tests::an_answer_overtaken_by_a_newer_request_is_dropped`, `editor::intelligence::tests::cmd_click_goes_to_the_declaration_or_chooses_between_several`, `editor::intelligence::tests::problems_are_underlined_and_the_hover_shows_them_with_the_documentation`, `editor::intelligence::tests::parameter_info_opens_after_a_parenthesis_and_follows_the_commas`, `editor::intelligence::tests::show_intentions_lists_the_code_actions_and_applies_the_chosen_one`, `editor::intelligence::tests::escape_closes_the_lookup_before_the_find_bar`. Missing, per the sweep (not re-checked): inlay hints, semantic tokens, code lens, document highlights and format are not wired natively (their models sit in unreached `editor_lsp` units).
- `point.registerKeyClaim:(at) src/features/vcs/diffDialog.tsx:85` → partial (see the run console section).

## Recipes not written, and why

- **Find Usages through the syntax tree** (`ipc:syntax_support`, `ipc:syntax_identifiers_in_files`, `ipc:syntax_identifiers`): not reachable by the sweep's recipe. Native `ask_syntax` (`workbench/code_insight.rs` ~273) runs only after `UsageSearch::answered` gets a non-cancelled `Err` from `lsp_references` (`code_insight/usage_search.rs` ~191-208). In Light mode with no server, `ide_host::ops::lsp::lsp_references` (`crates/ide-host/src/ops/lsp.rs` ~641-672) catches `ErrorKind::Unavailable` itself and answers `Ok` with a certain local's references or `text_usages::text_references` (approximate), so the `.js` project of the recipe never reaches `ops::syntax::*`, and a dropped comment hit could come from the backend's own text search (it takes `host.syntax()`), proving nothing about these three commands. Reaching them needs a server for a grammar language that fails with a non-`Unavailable` error: e.g. the fake server (`editor::intelligence::fake_server`) installed as a user language overriding `javascript` and answering `textDocument/references` with a malformed result (→ protocol error). Two unverified assumptions (a user `languages/javascript.toml` overrides the built-in while keeping its grammar; the malformed answer surfaces as a non-`Unavailable` error), so no test was written. The only other native caller is `editor/intelligence.rs` ~774 (`syntax_locals`, a different row). Rows stay as they are.
- `find.scrollToResults` recipe "the stored preference follows": dropped, the webview does not store it.

## Accessors / hunks the tests depend on

- None new beyond the `closeAllTabs` fix above. Every accessor used exists: `Workbench::{find_popup, find_bar, editor, tabs, tab_titles, tab_titles_of, bookmarks, recent_files, search_everywhere, status_items, open_menu, tool_window_view, active_markdown_host (pub(super), reachable from workbench::tests), run_driver}`, `FindPopup::{status, is_stale, lines, selected, menu, menu_lines}`, `FindBar::{match_count, set_search}`, `RunView::{find_open, find_field, find_counter, lines}`, `MarkdownView::{scroll_top, layout}`, `PreviewHost::{mode, preview}`, `Documents::{open_documents, identity}`; helpers `start`, `settle`, `search_project`, `trusted_project`, `run_project`, `program`, `popup`, `type_query`, `run_action`, `wait_long`, `close_backend_project` from the parent `mod tests`.

## Most likely to fail on first run

1. `close_all_tabs_closes_every_pane_and_leaves_nothing_to_reopen` — by design until the hunk lands (left pane keeps alpha and notes; `closed` not cleared). Likewise `the_run_consoles_find_bar_keeps_its_keys_from_the_ide_keymap` on its ⌘E assertion until the keep-the-keymap-out fix lands.
2. `a_markdown_preview_follows_the_editors_scrolling_and_the_hidden_editors_caret` — depends on the sync offset landing within 1px of the block top and on `MarkdownView::layout` holding the tops of all 1000 blocks; the selection also reveals the caret in the hidden editor, publishing a scroll as well as the caret move (Preview Only ignores the editor scroll per `PreviewSync::editor_scrolled`/`caret_moved`, and both target the same line anyway).
3. `the_run_consoles_find_bar_keeps_its_keys_from_the_ide_keymap` — assumes `/usr/bin/printf` (macOS and Linux) and that the counter after typing reads `n/m`; if the console also echoes the command line containing "hello", `total` is larger, which the test tolerates. If the platform keymap in tests is not IntelliJ macOS, F3/Escape compete with other bindings.
4. `go_to_class_finds_a_type_by_name_and_opens_it` — the Classes row text format is not pinned (asserts only "Widget" and not "make_widget"), and the index must have parsed `alpha.rs` (settle waits up to 3 s); the caret target (0, 11) assumes the symbol's selection range is the name, as the Symbols case in `search_everywhere_runs_actions_finds_text_and_switches_tabs` shows ((2,7) for `make_widget`).
5. `reload_with_encoding_reads_the_file_again_in_the_chosen_encoding` — assumes the menu opens with the selection on Reload with Encoding when it is enabled and that Right opens its submenu with the first encoding selected (as the existing test shows for Convert).
6. `the_find_in_files_popups_commands_move_the_selection_list_recent_searches_and_open_the_match` — assumes the popup starts with the first row selected and that the seeded `UiStorage` history is read when the list opens (`menu_items` reads `stored(cx, key)`); Escape must close only the list (as in `find::popup_view::tests::the_history_lists_the_recent_searches_and_replacements`).

# Part C — Editor search popups and refactoring

## proofs_editor_search: Editor search popups and refactoring (10 rows)

Worktree: /Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01 . No cargo was run; everything below about compiling or passing is unverified.

## mod lines

All files are children of the inline `mod tests` of `app-gpui/src/workbench/mod.rs`. Add, inside that module, right after its opening imports (anchor: the line `    use std::time::Duration;` that follows `    use gpui_kit::{TestAppContext, VisualTestContext};` directly under `mod tests {`), or next to the `mod proofs_editor_copy_reference;` line if the integrator already added one:

```rust
    mod proofs_editor_search_recent;
    mod proofs_editor_search_everywhere;
    mod proofs_editor_search_find;
    mod proofs_editor_search_safe_delete;
```

## File 1: app-gpui/src/workbench/tests/proofs_editor_search_recent.rs

Row: `unit:src/features/search/__tests__/dialogs.test.tsx` (stays partial, see the dialogs summary further down).

- `workbench::tests::proofs_editor_search_recent::extends_and_shrinks_a_keyboard_range_then_opens_its_files_in_order`: with every tab closed, Recent Files selects the first file, ↓ then ⇧↑ selects two rows (`chosen_files`), ⇧↓ shrinks the range to one, Enter opens both as tabs in list order (`tab_titles` == three.txt, two.txt) with the lead showing, and the popup closes. Spec case "Recent Files multi-selection › extends and shrinks a keyboard range, then opens its files in order".
- `...::starts_a_range_at_the_initially_selected_previous_file`: with three.txt in the editor the selection starts on two.txt (the previous file) and ⇧↓ selects two.txt and one.txt, not three.txt. Spec case "starts a range at the initially selected previous file".
- `...::handles_modifier_clicks_and_resets_on_filter_text_changes`: ⇧-click on row 2 selects 3 rows, a primary-modifier click on row 1 toggles it off, nothing opens; typing `.txt` in the filter leaves one row selected; a plain click on row 1 opens only that file and closes the popup. Spec case "handles modifier clicks and resets on filter text changes".

Risks: (0) in the click test, `.txt` is typed after two row clicks; hypothesis that a click on a row leaves the keyboard in the filter field (the view-level test never types after a click). (1) hypothesis: after `editor.closeAllTabs` the backend's recent list still has the three files in most-recent-first order (`ide_host::ops::nav::recent_files`); the first assert on `lines()` fails loudly if not. (2) The filter `.txt` must match all three rows of the speed search; if the matcher drops a row, `lines()[1]` still names what is clicked, so only the `len() == 1` assert could be affected. (3) `Modifiers::secondary_key(cx)` is called as in `everywhere::recent_view::tests::clicks_open_select_and_toggle`.

## File 2: app-gpui/src/workbench/tests/proofs_editor_search_everywhere.rs

- `workbench::tests::proofs_editor_search_everywhere::shows_recent_files_for_an_empty_query_then_flat_results` → `unit:src/features/search/__tests__/dialogs.test.tsx` (partial): with src/alpha.rs opened, All with an empty query lists `["[Recent Files]", "src/alpha.rs"]`; after typing `alph` no row is a `[Section]` header. Spec case "shows recent files for an empty query, then flat results; Enter runs an action"; the Enter-runs-an-action half is already proven by `workbench::tests::search_everywhere_runs_actions_finds_text_and_switches_tabs` (cite it with this test).
- `...::tab_cycles_scopes_and_repeating_the_scope_action_moves_to_all_keeping_the_query` → dialogs (partial): All →Tab→ Classes →⇧Tab⇧Tab→ Text; typing `kept` then running `navigate.searchEverywhere` again moves the same popup to All with `kept` kept. Spec case "⇥ cycles scopes and repeating the scope action toggles non-project items", minus the include-non-project half (not built, see below).
- `...::reports_state_only_for_actions_with_toggled` → `unit:src/features/search/__tests__/toggleActions.test.ts` (1 case → done): registers a toggle action (`Action::toggled`) and a plain one in the registry; `action_toggled` reads `Some(false)` and `None`; the toggle run from Find Action (typed, Enter) reads `Some(true)` and the popup stays open; the plain one runs once, reads `None` and closes the popup.

Risk: hypothesis that the backend's recent list (`ide_host::ops::nav::recent_files`) holds exactly src/alpha.rs in a fresh home; if the project open records another file the exact `assert_eq!` on lines fails (relax it to `starts_with(&["[Recent Files]"])` + `contains`).

Difference from the webview (not enshrined): the Search Everywhere action row draws no on/off switch for a toggle action (`everywhere/view.rs` render of `SeItem::Action` shows icon, title and category only), although `ActionMeta::toggle` says "Search Everywhere shows an ON/OFF switch". Not asserted either way.

## File 3: app-gpui/src/workbench/tests/proofs_editor_search_find.rs

- `workbench::tests::proofs_editor_search_find::streams_results_and_highlights_the_matches_while_replacing` → dialogs (partial): Replace in Files on `needle` counts "1 match in 1 file"; with the replacement `item` the preview of notes.txt still marks `(4, 10)` on line 1, `preview_mode()` is off and `diff_lines()` is empty (no del/ins), the Replace label is singular, `find-popup-replace-all` is drawn, and notes.txt is unchanged on disk. Spec case "Find in Files dialog › streams results and highlights the matches while replacing, as IDEA". Gap: the webview asserts Replace All is _enabled_; there is no accessor for the button's enabled state (`popup_view.rs` computes `enabled` inline in render). Optional hunk to make it assertable, in `app-gpui/src/find/popup_view.rs` next to `replace_label`:
  ```rust
      /// Whether Replace All can run now.
      pub fn can_replace_all(&self) -> bool {
          !self.is_stale() && !self.rows.is_empty() && self.running.is_none() && self.status() != FindStatus::Searching
      }
  ```
  (render computes `enabled = !stale && !self.rows.is_empty() && !busy && !searching` with `busy = self.running.is_some()` and `searching = status == FindStatus::Searching`, popup_view.rs ~2629; check that render's `stale` is `self.is_stale()`).
- `...::marks_the_matches_of_the_previewed_file_and_its_current_line` → `unit:src/features/search/__tests__/codePreview.test.tsx` (partial, 1 of 8 cases): two result rows; the preview is on line 0 (`preview_line`), marks `(0,6)` on line 0, none on line 1, `(0,6),(11,17)` on line 2; ↓ moves the preview's line to 2. Spec case "marks matches (the current line's as current) and the current line", minus "current" vs other matches (`preview_marks` does not say which marks are current; no accessor). Proof text: "`…::marks_the_matches_of_the_previewed_file_and_its_current_line`; 1 of 8 cases: the preview is a read-only line list (FilePreview, popup_view.rs), so the editable preview, its shared undo/save with a tab, the read-only kinds and the Enter/Escape cases are not built".
- `...::keeps_explicit_directory_path_precedence_and_a_query_only_argument` → `unit:src/features/search/__tests__/pathScopeActions.test.tsx` (partial): for `find.inPath` and `find.replaceInPath`, `{directory, path, query}` gives Directory + `directory` + the query; `{path}` gives that path; `{query}` keeps the directory and sets the pattern. Spec case "keeps explicit directory/path precedence and a query-only argument" (×2), except that the webview's query-only call falls back to the _focused_ directory, which the native app never publishes (see the next test).
- `...::keeps_the_previous_scope_when_no_path_tree_owns_focus` → pathScopeActions (partial): with the editor focused, the action without argument reopens on the Directory scope it was given, and on Project once Project was chosen (`select_scope`). Spec case "keeps the previous scope when no path tree owns focus" (×2).
- `...::takes_the_focused_directory_of_the_project_view` → pathScopeActions: **expected to fail now.** A folder selected in the focused Project view must become the Directory scope (spec case "takes the focused directory through %s shortcuts", run here through the registry rather than both presets' keys). Measured (grep in this session): nothing in `app-gpui/src` writes the context key `focusedDirectory`; only `find/store.rs::parse_find_arg` reads it. Fix, in `app-gpui/src/workbench/mod.rs` `fn publish_context`, after the line `        set_context(window, cx, "projectViewFocus", focused_panel.as_deref() == Some(PROJECT_TOOL_WINDOW));`:
  ```rust
          let focused_directory = (focused_panel.as_deref() == Some(PROJECT_TOOL_WINDOW))
              .then(|| self.project_tree().and_then(|tree| tree.read(cx).selected_path().map(str::to_owned)))
              .flatten();
          match focused_directory {
              Some(directory) => set_context(window, cx, "focusedDirectory", directory),
              None => crate::platform::context::clear_context(window, cx, "focusedDirectory"),
          }
  ```
  (the webview's Tree gives a file row's own path, `src/ui/Tree.test.tsx`; the Project view's `getDirectory` decides whether a file stands for its folder — read `src/features/project/` before choosing). With that, the bookmarks case (folders, files and line bookmarks, not lists or editor focus) needs the same key from the Bookmarks window; it is not written.

pathScopeActions has 2 × 4 = 8 cases; with the fix, 6 are covered here (the two shortcut-preset variants are run once through the registry each), the bookmarks pair is not. Stays partial.

Risks: `searched` waits for `FindStatus::Done && !is_stale()`; after `set_replacement` no new search may start, which is fine. `preview_marks` covers every match line of the selected row's file (`file_highlights`, popup_view.rs ~1778, read in this session), so the line-2 assert before ↓ is sound.

## File 4: app-gpui/src/workbench/tests/proofs_editor_search_safe_delete.rs

Own fixture (`usages_project`): the shared `start()` runs `Backend::init` before a language could be installed, and the fake server of `editor::intelligence::fake_server` advertises neither `documentSymbolProvider` nor `referencesProvider` (read in this session), so the file carries its own small Python LSP (`SERVER`) with those two capabilities plus the same answers/delays/log scheme, installs the `mock` language under `AppDirs::under(home).config/languages`, sets Full mode, trusts the project, opens Widget.mock in the editor and waits for `textDocument/didOpen` in the server's log (so the server is up and the backend holds the document). Needs `python3` on PATH, as the intelligence tests do.

- `workbench::tests::proofs_editor_search_safe_delete::find_usages_on_a_file_lists_the_usages_of_its_symbols_from_other_files` → `module:src/editor/refactor/safeDelete.ts` (its `fileUsages` + `viewUsages`): right-click Widget.mock › Find Usages runs `FileUsagesFlow` (didOpen, documentSymbol, references) and the Find Usages window must list User.mock. **Expected to fail now**, measured by reading the code in this session: the file flow's `Effect::ViewUsages` is carried out in `app-gpui/src/workbench/project_files.rs` as `serde_json::json!({ "name": name, "usages": usages })`, but `find.showUsageResults` parses `{ symbol, locations }` (`code_insight/usages.rs` `ShowUsageResults::parse` returns `None` without `locations`), and the webview sends `{ symbol, locations: usages }` (`src/editor/refactor/safeDelete.ts` `viewUsages`). The action fails (logged), nothing is shown, the test times out in `wait_long`. Fix (also fixes Safe Delete › View Usages from the project view):
  ```diff
               Effect::ViewUsages { name, usages } => {
  -                let arg = serde_json::json!({ "name": name, "usages": usages });
  +                let arg = serde_json::json!({ "symbol": name, "locations": usages });
                   self.run_flow_action("find.showUsageResults", arg, window, cx);
  ```
- `...::a_safe_delete_of_a_used_file_asks_with_the_usages_detected_question` → `module:src/editor/refactor/safeDelete.ts` (`askUsagesDetected`): Delete… with safe delete on, Enter; the usage search finds User.mock's reference and the window shows a `PromptDialog` titled "Usages Detected" with buttons [View Usages (primary), Delete Anyway]; Escape closes it, the notification "Some items could not be deleted" never comes, User.mock stays. Trash safety: Widget.mock is removed from disk while the Delete dialog is open, before Enter, exactly as `a_confirmed_delete_searches_deletes_and_reports_what_could_not_be_deleted` does. The usage search still works because `ide_host::ops::doc::open` → `Documents::open` → `reopen` returns the already-open document by path without reading the disk (`crates/ide-workspace/src/documents.rs` `lookup`/`reopen`, read in this session). Every path that goes on to delete (an `Unsupported` or failed search included: `doc_outline` falls back to the syntax outline and an empty outline ends the flow `Unsupported`, which proceeds to delete) then deletes a file that is gone, which the backend refuses before the Trash. Gap: the question's body ("1 usage found / File 'Widget.mock' has 1 usage that is not safe to delete.") is not asserted: `PromptDialog` has no accessor for its message. Optional hunk in `app-gpui/src/vcs/prompt_dialogs.rs`, after `pub fn title`:
  ```rust
      /// The message under the title, when there is one.
      pub fn message(&self) -> Option<&str> {
          self.message.as_deref()
      }
  ```
  (hypothesis: the field is `message: Option<String>`, as `Spec { message: Some(request.body), .. }` in `PromptDialog::ask` suggests; check its declaration.)

Row status: `module:src/editor/refactor/safeDelete.ts` stays **partial** even with the fix: these prove the project view's file flow (`fileUsages`, `askUsagesDetected`, `viewUsages`) only. The editor's Safe Delete of a symbol (`safeDelete`, `SafeDeleteFlow` in `editor_refactor/safe_delete.rs`) has no caller outside `editor_refactor` (measured: `grep -rn "editor_refactor::" app-gpui/src` lists only `project_view/file_dialogs.rs` (UsagesQuestion) and `workbench/project_files.rs` (FileUsagesFlow, FileUsagesAction)), and no `refactor.safeDelete` handler is registered.

Risks: (1) hypothesis: `document_symbols` and `lsp_references` reach the mock server for the open document; if not, test 1 times out (harmless) and test 2 finds no usages and the delete of the missing file is refused (no Trash; the test then fails waiting for the question). (2) Removing Widget.mock while its document is open sends a file event; hypothesis that the backend keeps the document registered (if it closes it, `reopen` misses, `Documents::open` fails on the missing file, the flow ends `Unsupported`: safe, but the test fails). (3) The right-click menu is clicked by `menu-row-0-{index}` where index counts separators, as `PopupMenu::lines` does.

## Rows not given a test, and why

- `module:src/editor/refactor/catalog.ts`: not reachable. Measured: `grep -rn "editor_refactor::" app-gpui/src` outside `editor_refactor/` names only `safe_delete::{UsagesQuestion, FileUsagesAction, FileUsagesFlow}`; `catalog` is used by `refactor_this.rs`, `server_introduce.rs` and `safe_delete.rs::cannot_perform`, and `cannot_perform` is only called in the editor `SafeDeleteFlow` (lines ~660, ~858), which nothing outside `editor_refactor` calls; no `refactor.*` action handler is registered (`grep -rn '"refactor\.' app-gpui/src` hits only `editor_refactor/`, `menu_spec.rs` and `vim/rc.rs`). The sweep recipe ("the catalog marks a refactoring the server cannot perform as unavailable") has no UI to assert. Belongs in the not-built list.
- `module:src/editor/refactor/index.ts`: a barrel re-exporting refactorThis (not wired, see above), safeDelete (partial here) and hierarchy/HierarchyDialog (not built). Stays `[ ]`, or `[~]` citing the two safe-delete tests once they pass.

## dialogs.test.tsx: case count

The spec declares 22 cases (27 instances with the each-tables): Search Everywhere dialog 8; SE multi-selection 8 (11 instances); Recent Files multi-selection 3; Switcher 1; Find in Files 2 (4 instances). Covered, 8 of 22:

- written here: empty-query recent files + flat results (SE 1, its Enter half by `workbench::tests::search_everywhere_runs_actions_finds_text_and_switches_tabs`); ⇥ cycling + repeated action keeps the query (SE 5, half); Recent Files ×3; Find in Files streams + highlights while replacing (1, minus "enabled").
- already proven: "/ lists scope commands; picking one or typing '/files q' switches the tab" by `workbench::tests::search_everywhere_runs_actions_finds_text_and_switches_tabs` (`/` list, `/files beta`) and `workbench::tests::search_everywhere_gives_the_keyboard_back_and_runs_actions_where_it_was_opened` (`/fi` + Enter → Files, empty query); Switcher "opens the selection when Ctrl is released" by `workbench::tests::recent_files_and_the_switcher_go_between_the_editors` (the focus half — the editor left behind does not get focus back first — is not asserted there).
- not built (measured by reading `everywhere/view.rs` in this session): the Preview toggle (no preview in the view); ⌥↑/⌥↓ history (`claim_key` swallows `up`/`down` with Alt; `everywhere::store::search_history` is never called by the view); the Actions tab's "Include disabled actions" checkbox and ⌥↵ assign-shortcut (`"enter"` with Alt/⌘ is swallowed); the Files funnel filter (`SeEnv.languages: None`, no filter UI); search everywhere when nothing is found (`include_non_project`/`widen` live only in `everywhere::store`, unused by the view; the repeat-on-All toggle likewise); all 11 SE multi-selection instances (the view keeps one `active` row, no range or toggle); Find in Files shortcut spelling per OS (3 instances: `key_label` uses `is_mac_host()`, the host OS, which a test cannot switch).
  Proof text: "…; 8 of 22 cases: the Preview toggle, ⌥↑/⌥↓ history, the Actions checkbox and ⌥↵, the file-type funnel, searching everywhere when nothing is found and Search Everywhere's multi-selection are not built, and shortcut spelling follows the host OS". Stays partial.

## Feature rows (notes only, no test)

Measured by grep in this session; the sweep's "wired" claims for templates and vim are wrong.

- `feature:code-insight`: partial. App-level tests exist: `workbench::tests::the_problems_window_lists_published_problems_and_opens_the_chosen_one`, `workbench::tests::find_usages_without_a_server_lists_text_matches_by_file_and_show_usages_opens_the_chosen_one`, `workbench::tests::type_hierarchy_without_a_server_says_no_language_server_is_available`, `workbench::tests::the_code_insight_actions_and_events_run_in_a_window_with_a_project` (find usages, rerun, show usage results, show usages, call hierarchy, server status item). Not yet: workspace undo (per the sweep; not checked here), file Find Usages (blocked by the `{name, usages}` bug above).
- `feature:templates`: cannot be partial on app tests. `code.insertLiveTemplate` and `code.surroundWith` appear only in `workbench/menu_spec.rs` (and `vim/rc.rs`'s name table); no handler is registered; `templates::editor` (live templates, postfix, surround) is not called outside `templates/`. The only app use of `templates` is `templates::snippet` from `editor_lsp/completion.rs` (LSP snippet insertion). Model tests only: `templates::editor::tests` (e.g. `expands_a_live_template_abbreviation`, `expands_postfix_templates_replacing_the_expression`, `wraps_the_selected_lines`, `tab_tries_postfix_then_live_templates_then_emmet`), `templates::emmet_model::tests`. Row stays `[ ]` (or a model-only note).
- `feature:vim`: cannot be partial on app tests. `vim.toggle` appears only in `menu_spec.rs`; `VimMode::new` is called only in `features_a/vim/mode.rs` tests; `status_items::vim_item` is called only by its own test. The two tests the sweep cites are plain `#[test]` model tests (`features_a::vim::mode::tests::vim_toggle_turns_vim_on_and_off_for_every_editor_live_and_persists_it`, `workbench::status_items::tests::the_vim_item_shows_the_mode_while_on_and_a_faint_name_while_off`). Row stays `[ ]` (or model-only).

## Row summary

- done: `unit:src/features/search/__tests__/toggleActions.test.ts` (1 test, 1 case).
- partial: `unit:src/features/search/__tests__/dialogs.test.tsx` (6 new tests + 3 existing), `unit:src/features/search/__tests__/codePreview.test.tsx` (1 of 8), `unit:src/features/search/__tests__/pathScopeActions.test.tsx` (3 tests, one needs the `focusedDirectory` fix), `module:src/editor/refactor/safeDelete.ts` (2 tests, one needs the `{symbol, locations}` fix), `feature:code-insight` (existing tests).
- not provable now: `module:src/editor/refactor/catalog.ts`, `module:src/editor/refactor/index.ts`, `feature:templates`, `feature:vim`.

## Most likely to fail on first run

1. `takes_the_focused_directory_of_the_project_view`: by design until the `focusedDirectory` hunk lands.
2. `find_usages_on_a_file_lists_the_usages_of_its_symbols_from_other_files`: by design until the `{symbol, locations}` hunk lands; also the mock-server risks above.
3. `a_safe_delete_of_a_used_file_asks_with_the_usages_detected_question`: mock server readiness and the file removed mid-flow.
4. `shows_recent_files_for_an_empty_query_then_flat_results` and the Recent Files tests: exact contents of the backend's recent list.
5. `handles_modifier_clicks_and_resets_on_filter_text_changes`: the speed search's matching of `.txt` and click delivery to `recent-file-N` rows.
6. Compile nits only an integrator can see: `Modifiers::secondary_key(cx)` argument type, `Entity::update_in` on `FindPopup` with `select_scope`, and rustfmt line breaks on the ~118-column lines.

# Part D — highlighting, command_actions/editing_tests, find masks, navigation modifier, copyReference, breadcrumbs

## proofs_editor_misc — notes

Worktree: /Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01 . All paths below are relative to it. Nothing here was compiled or run.

## Rows found not reachable (no test written; belong in list C)

- `unit:src/editor/core/view/statusItems.test.tsx` — the breadcrumb item is not drawn. `status_items::BREADCRUMBS` ("editor.breadcrumbs") is only declared in `BUILT_INS` (app-gpui/src/workbench/status_items.rs:20,47); it is not in `FED_STATUS_ITEMS` (workbench/chrome.rs:47) and nothing feeds it text; `editor_model::nav_bar` has no caller outside its module. Measured with `grep -rn "BREADCRUMBS\|nav_bar" app-gpui/src`.
- `module:src/editor/core/colorSchemeRuntime.ts` — a chosen scheme does not reach the editor's syntax colours. The editor paints `theme::color(cx, syntax_token(kind))` (editor/view.rs:1858); `theme::color_scheme::scheme_style` / `scheme_css` (which turn `editor.colorScheme` + overrides into `--syn-*` values) have callers only in tests (theme/color_scheme.rs, theme/themes.rs, theme/icls.rs). Measured with `grep -rn "scheme_style\|scheme_css" app-gpui/src`.

## Mod lines to add (5 hunks, 10 lines)

1. app-gpui/src/editor/view.rs, inside `mod tests {` (~2072), after the anchor `    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};` (~2075, unique):

```
    mod proofs_editor_highlight_backend;
    mod proofs_editor_highlight_cmake;
    mod proofs_editor_highlight_interpolation;
    mod proofs_editor_highlight_shell;
    mod proofs_editor_highlight_support;
```

2. app-gpui/src/editor/view.rs, inside `mod editing_tests {` (~2346), after the anchor `    use crate::editor_commands::change;` (~2349, unique):

```
    mod proofs_editor_clipboard_history;
    mod proofs_editor_multi_caret;
```

3. app-gpui/src/find/popup_view.rs, inside `mod tests {` (~2824), after the anchor `    use std::cell::RefCell;` (~2829): `    mod proofs_editor_project_masks;`
4. app-gpui/src/workbench/mod.rs, inside `mod tests {`, after its first `    use std::time::Duration;` (~3736 at the time of writing; the file is moving): `    mod proofs_editor_copy_reference;`
5. app-gpui/src/editor/intelligence.rs, inside `mod tests {` (~1608), after the anchor `    use std::path::PathBuf;` (~1617, unique): `    mod proofs_editor_navigation_modifier;`

## Suggested parity.json entries (only once the cited tests are green; the predicted-red tests are not cited)

- `"ipc:index_file_extensions"`: done, "`find::popup_view::tests::proofs_editor_project_masks::the_masks_list_offers_the_projects_most_common_language_file_types_after_the_recent_ones`"
- `"ipc:languages"`: same proof. Measured: src-tauri/src/state.rs:86–92 builds one `Arc<LanguageRegistry>` and passes it to `Host::new` as `languages`, so the adapter's `state.languages.all()` and the native `host.languages.all()` read the same registry; but the adapter is not an `ide_host::ops` function, so the brief's IPC bar is not met literally. Done vs partial is the integrator's call.
- `"module:src/editor/core/copyReference.ts"`: partial, "`workbench::tests::proofs_editor_copy_reference::copy_reference_copies_the_qualified_name_of_the_declaration_at_the_caret`; through Copy Path/Reference… only: the Copy Reference action (⌥⇧⌘C) is not registered"
- `"unit:src/editor/core/cm/multiCaret.test.ts"` (6 cases): partial, "`editor::view::editing_tests::proofs_editor_multi_caret::each_preset_keeps_the_default_gesture`, `…::matches_the_complete_stroke_and_only_the_left_button`, `…::uses_rebinding_immediately_and_yields_column_selection_to_the_caret_gesture`, `…::honours_removal_of_the_caret_gesture_and_restores_it`, `…::matches_normalized_ctrl_alt_strokes_as_reported_for_right_alt_on_windows`; 5 of 6 cases: ⌥-drag and middle-button column selection are not built"
- `"module:src/editor/core/clipboardHistory.ts"`: partial, "`editor::view::editing_tests::proofs_editor_clipboard_history::copies_and_cuts_are_recorded_newest_first_and_the_latest_line_copy_pastes_above`, `…::the_clipboard_history_keeps_the_latest_thirty_copies`; Paste from History and copies outside editors are not wired"
- `"unit:src/editor/core/cm/syntaxHighlight.test.ts"` (2 cases): partial now, "`editor::view::tests::proofs_editor_highlight_backend::lets_kotlin_declaration_roles_override_the_stream_lexers_generic_definition_style`; 1 of 2 cases: highlight ranges do not move with edits"; done once `…::styles_backend_ranges_with_the_active_highlight_style_and_maps_them_through_edits` is green.
- `"unit:src/editor/core/cm/cmake.test.ts"` (7), `"unit:src/editor/core/cm/shellInterpolation.test.ts"` (4): cite the green tests of `editor::view::tests::proofs_editor_highlight_cmake` / `…_shell` as partial with "; N of M cases: …"; done when all are green.
- `"unit:src/editor/core/cm/interpolation.test.ts"` (8 cases): at most partial, "…; Groovy has no grammar and the mode lookup by name or alias has no native counterpart (2 cases), Kotlin/Scala/Dart templates are not highlighted"
- `"module:src/platform/navigationModifier.ts"`: partial, "`editor::intelligence::tests::proofs_editor_navigation_modifier::the_plain_hover_yields_while_the_navigation_modifier_is_held`; from pointer events only: a key press, a key release and window blur are not followed, and there is no ⌘-hover link"

## Predicted-red tests (behaviour differs from the webview; not breakage)

`workbench::tests::proofs_editor_copy_reference::the_copy_reference_action_copies_the_qualified_name_without_a_popup`; `editor::view::editing_tests::proofs_editor_multi_caret::restores_alt_drag_column_selection_once_the_caret_gesture_is_removed`; `editor::intelligence::tests::proofs_editor_navigation_modifier::pressing_the_navigation_modifier_closes_an_open_hover`; `editor::view::tests::proofs_editor_highlight_backend::styles_backend_ranges_with_the_active_highlight_style_and_maps_them_through_edits`; in `proofs_editor_highlight_cmake`: all but `keeps_multiline_bracket_comments_and_literal_bracket_strings_opaque` (and possibly the reparse part of the last one); in `proofs_editor_highlight_shell`: the first three; in `proofs_editor_highlight_interpolation`: `separates_delimiters_names_operators_and_numbers`, `keeps_nested_braces_strings_and_comments_inside_expressions`, `preserves_literal_dollars_and_braces` (their Kotlin/Scala/Dart parts) and `handles_kotlin_multiline_raw_templates_and_nested_templates`. Each failure's cause and fix is in its section below.

## Per-file details

### File 1: app-gpui/src/find/popup_view/tests/proofs_editor_project_masks.rs (1 test)

Mod line: hunk 3 above.

- `the_masks_list_offers_the_projects_most_common_language_file_types_after_the_recent_ones` → `ipc:index_file_extensions` (done) and `ipc:languages` (done or partial: see the suggested entries). Asserts the masks menu reads `*.py, *.md, *.rs, *.go` for a project of 3 `.rs`, 2 `.py`, 1 `.go` and 4 `.zzq` files with recent masks `*.py, *.md`: the order by file count can only come from the index's counts (`ide_host::ops::nav::index_file_extensions`, the same function src-tauri/src/commands/nav.rs:21 calls) and the missing `*.zzq` (the most numerous) only from the languages' extensions (`host.languages.all()`; the `languages` adapter, src-tauri/src/commands/app.rs:154, reads `state.languages.all()`, the same `Arc<LanguageRegistry>` that src-tauri/src/state.rs:86–92 hands to `Host::new`, measured; it is not an `ide_host::ops` function). Choosing `*.rs` puts it in the mask field and the query. The webview (src/features/search/find/store.ts `refreshProjectMasks`, called once when Find in Files opens) does the same: filter by language extensions, top 5, recent masks first, no duplicates.
- Waits for the index with `ide_host::ops::nav::index_file_extensions` before opening the popup, because the popup asks once at construction (as the webview does on mount); without the wait the menu would be recent-only.
- Not written: `module:src/platform/textSearch.ts`. Streaming is already shown by `find::popup_view::tests::searches_as_you_type_once_the_query_has_rested` (results arrive, the next query replaces them), which can be cited for `[~]`; that a second query cancels the first (`host.cancel_request`, popup_view.rs:648/1101) is not observable without a search slow enough to overlap, which a temp project cannot make deterministic.

### File 2: app-gpui/src/workbench/tests/proofs_editor_copy_reference.rs (2 tests)

Mod line: hunk 4 above. The directory `app-gpui/src/workbench/tests/` did not exist when this was written; other agents may add files to it.

- `copy_reference_copies_the_qualified_name_of_the_declaration_at_the_caret` → `module:src/editor/core/copyReference.ts` (partial). Runs `editor.copyPathReference` through the registry, waits for the Reference row to resolve (outline from `ide_host::ops::doc::doc_outline`), chooses it with the keyboard, and asserts the clipboard and the notice: `com.acme.Orders#place` (Java method, package from the text, `declaredName` drops `(int)`), `src/main/java/com/acme/Orders.java:4` off a name (`referenceOffset` / fallback), `crate::orders::place` (Rust `rustModulePath`). Partial because the direct action below is missing, and Python/Go/Kotlin/C# conventions are only covered by the model tests `editor_commands::copy_reference::tests`.
- `the_copy_reference_action_copies_the_qualified_name_without_a_popup` → same row; EXPECTED TO FAIL: the webview registers `editor.copyReference` as its own action (src/editor/core/actions.ts:722, Edit › Copy Reference, ⌥⇧⌘C) that copies without a popup; natively the id exists only as the popup row's id (project_view/copy_path_popup.rs:23) and in menu_spec.rs:292/319 — no `registry.handle("editor.copyReference", …)` (measured: `grep -rn '"editor.copyReference"' app-gpui/src`). `run_action` panics with "editor.copyReference runs: …". Fix: in workbench/project_files.rs `register`, next to `editor.copyPathReference` (line ~271), register `editor.copyReference` to resolve the reference exactly as `show_copy_path_popup` does (outline → `symbol_chain` → `qualified_reference`) and write it to the clipboard with the `copied_hint(.., REFERENCE, ..)` hint; a string argument is copied as is (the webview's `arg`).

### File 3: app-gpui/src/editor/view/editing_tests/proofs_editor_multi_caret.rs (5 tests)

### File 4: app-gpui/src/editor/view/editing_tests/proofs_editor_clipboard_history.rs (2 tests)

Mod lines: hunk 2 above.
They use `editing_tests`' private `Opened`, `open`, `text`, `selections` and view.rs's private `ADD_CARET_ACTION` through `use super::*`.

multiCaret → `unit:src/editor/core/cm/multiCaret.test.ts` (6 cases; 6 tests covering 5 cases, named by the spec sentences; the fourth case is split into a green and a red test; stays PARTIAL):

- `each_preset_keeps_the_default_gesture` — for every preset in `PRESET_TITLES`, `bindings_for("editor.addOrRemoveCaret")` is `["Alt+Shift+Button1"]` and ⌥⇧-click adds a caret.
- `matches_the_complete_stroke_and_only_the_left_button` — ⌥⇧-click adds; ⌥, ⌃⌥⇧, ⌘⌥⇧ clicks and a middle-button ⌥ press add no caret (one range; the middle press is not asserted to leave the caret, since the webview starts a column selection there).
- `uses_rebinding_immediately_and_yields_column_selection_to_the_caret_gesture` — overrides `Alt+Button1`: ⌥⇧ no longer adds, ⌥-click adds `[(0,0),(2,2)]`, ⌥-drag 2→8 gives `[(0,0),(2,8)]`, ⌥-click on the caret at 2 removes it.
- `honours_removal_of_the_caret_gesture_and_restores_it` — with the binding removed ⌥⇧-click and ⌥-click add no caret; cleared overrides bring ⌥⇧-click back. Predicted green.
- `restores_alt_drag_column_selection_once_the_caret_gesture_is_removed` — EXPECTED TO FAIL: ⌥-drag from offset 1 to 16 must select columns 1–3 of each line `[(1,3),(7,9),(14,16)]` (the webview's CodeMirror rectangular selection, which the spec checks with `styles(event())[1]`). Natively `Editor::mouse_down` / `mouse_move` (editor/view.rs:1474–1553) have no ⌥-drag column selection; a ⌥-drag is a plain drag, giving `[(1,16)]`. Fix: in `mouse_down`, when the press is ⌥ + left (and not the add-caret binding), start a column drag; in `mouse_move`, set one range per line between the press's and the pointer's line, each spanning the two UTF-16 columns (clamped to the line), as `extend_column` (view.rs:1334) does for ⇧-arrows.
- `matches_normalized_ctrl_alt_strokes_as_reported_for_right_alt_on_windows` — override `alt+ctrl+button1`: ⌃⌥-click adds, ⌥-click does not.
- Not written: "preserves middle-button column selection except on Linux": native has no middle-button column selection (the editor listens to `MouseButton::Left` only, view.rs:1724), and the host OS cannot be switched in a test. Not built.

clipboardHistory → `module:src/editor/core/clipboardHistory.ts` (stays PARTIAL):

- `copies_and_cuts_are_recorded_newest_first_and_the_latest_line_copy_pastes_above` — ⌘C/⌘X through the keymap record `[beta, alpha, gamma(line-wise)]` newest first with no repeat; a caret-line copy pastes above the caret's line (`isLinewiseClipboardText`); text written to the clipboard from outside pastes at the caret.
- `the_clipboard_history_keeps_the_latest_thirty_copies` — 35 copies keep the newest 30 (`MAX_CLIPBOARD_ENTRIES`).
- Missing for done: Edit › Paste from History (`code.pasteFromHistory`) has no handler natively (only menu_spec.rs:285/312 name it; measured with grep), so the history is not visible to a user; `installClipboardTracking` (copies outside editors go into the history) has no native counterpart; `clipLabel` is only model-tested (`editor_commands::clipboard_history::tests`). The history is asserted through the `EditorClipboard` global, the only place it shows.

`module:src/editor/core/actions.ts`, `module:src/editor/core/backend.ts`, `module:src/editor/core/index.ts`: no test written; they stay `[~]`. Audit (measured: ids quoted in src/editor/core/actions.ts lines 600–1207 grepped as literals in app-gpui/src): 91 of 97 ids appear; missing entirely: `code.collapseRecursively`, `code.expandRecursively`, `editor.scrollToCenter`, `editor.toggleRainbowBrackets`, `editor.toggleSemanticHighlighting`, `project.refresh`. A literal is not a handler: `editor.copyReference` and `code.pasteFromHistory` appear but have no `registry.handle` (see File 2 and above), so the real gap is larger; a registration-level diff (ids passed to `registry.handle`/`handle_toggle` vs the TS `registerActions` table) is the next step before any of the three can be done.

### Files 5–9: highlighting, under app-gpui/src/editor/view/tests/ (19 tests + a support module)

Mod lines: hunk 1 above.

How they compare: every expectation is the colour token the user sees. Each spec tag is mapped through src/editor/core/cm/highlight.ts (`openideaHighlightStyle`) to its `--syn-*` token and compared with `syntax_token(kind)` of the span the editor paints (editor/view.rs:55), character by character, skipping blanks, so a range split or merged differently still compares equal. `proofs_editor_highlight_support` collects every mismatch of a test and panics once with all of them and the painted runs, so one run reports the whole difference. A needle written `before«inner»after` checks `inner` in that context. Highlights come from `ide_host::ops::doc::doc_highlights` (view.rs:1259), the same function the `doc_highlights` adapter calls (src-tauri/src/commands/doc.rs).

`unit:src/editor/core/cm/cmake.test.ts` (7 cases, 7 tests in proofs_editor_highlight_cmake.rs, named by the spec sentences). Native highlighting for CMake is tree-sitter-cmake 0.7.5's own query (`grammars::native_highlights("cmake")`). Predicted from reading that query (hypotheses; the run will say):

- `styles_the_screenshots_commands_argument_keywords_and_built_in_variables` — EXPECTED FAIL: commands → `syn-builtin` pass (`@function.builtin` list) and `ON` → keyword passes, but `VERSION`/`LANGUAGES`/`NAME`/`COMMAND` are `@constant` (syn-constant, spec: keyword), `CMAKE_CXX_STANDARD` etc. are `@variable` as `set`'s first argument (spec: built-in), `3.20`/`17` are not captured at all (spec: number).
- `styles_version_ranges_and_the_project_ctest_and_cpack_predefined_variables` — EXPECTED FAIL (no numbers; `PROJECT_VERSION` variable).
- `recognizes_case_insensitive_commands_control_flow_and_user_defined_functions` — EXPECTED FAIL only on the definition: `Build` in `FUNCTION(Build …)` is `@function` → syn-function-call, spec: definition → syn-function. Keywords, `MESSAGE`, the call `Build(app)` predicted to pass.
- `separates_quoted_text_nested_variable_expansions_environment_cache_lookups_and_escapes` — EXPECTED FAIL on the expansions: `$`, `{`, `}` are `@punctuation.special` (syn-punct), the name `@variable`, `ENV`/`CACHE` `@module` (syn-type); spec paints each whole expansion as `special(variableName)` → syn-constant. Escape, no comment, `# text` as string predicted to pass.
- `keeps_multiline_bracket_comments_and_literal_bracket_strings_opaque` — predicted pass.
- `handles_nested_generator_expressions_without_swallowing_following_commands` — EXPECTED FAIL on the generator expression (not captured; spec: one constant span); `message` predicted to pass.
- `reparses_multiline_string_edits_and_resolves_cmake_filename_associations` — the edit is applied through `Editor::apply_edit` and the new highlights awaited (`highlights_version == version()`); predicted pass for the reparse (hypothesis: depends on tree-sitter's recovery of the unterminated quoted argument) and for `CMakeLists.txt` / `module.cmake`; EXPECTED FAIL for `toolchain.cmake.in` (languages/cmake.toml lists only `extensions = ["cmake"]`; asserted through `host.languages.detect`).
- Fix for the CMake failures: give the cmake Spec its own query (crates/ide-language/queries/cmake/highlights.scm, `Spec.highlights`) that captures argument keywords as `@keyword`, predefined `CMAKE_*`/`PROJECT_*`/`CTEST_*`/`CPACK_*` variables as `@variable.builtin`-like built-in kind (a capture painted `syn-builtin`, e.g. `@type.builtin` maps to Builtin), numbers/version ranges as `@number` (`#match?` on unquoted arguments), whole `variable_ref`/generator expressions as `@constant`, the defined function name as `@function.definition`; and add `.cmake.in` detection.

`unit:src/editor/core/cm/shellInterpolation.test.ts` (4 cases, 4 tests in proofs_editor_highlight_shell.rs). Native is tree-sitter-bash 0.25.1's own query:

- `separates_variables_command_substitutions_and_arithmetic_inside_strings` — EXPECTED FAIL nearly throughout: the `$` is `@operator` (syn-punct), `${`/`}`/`$(`/`)`/`$((`/`))` are inside `@embedded` and uncoloured (spec: `special(brace)` → syn-decorator), names are `(variable_name) @property` (syn-property, spec: variable), arithmetic numbers and `+` uncaptured, `echo` is `(command_name) @function` (syn-function-call, spec: `standard(variableName)` → syn-builtin).
- `highlights_parameter_operators_without_treating_length_expansion_as_a_comment` — EXPECTED FAIL (`#`, `:-` uncoloured, `name` property); "no comment" passes.
- `keeps_nested_command_substitutions_and_quoted_strings_independent` — EXPECTED FAIL on the delimiters; ` after"` as string passes.
- `leaves_single_quotes_escaped_dollars_comments_and_heredocs_intact` — predicted pass (nothing is ever syn-decorator natively). The four spec documents are one file here; the assertion (no delimiter colour anywhere) is the spec's.
- Fix: a bash query of our own (queries/bash/highlights.scm on the bash Spec) capturing expansion / substitution / arithmetic delimiters to a kind painted syn-decorator (today only `Attribute`/`Macro` paint `syn-decorator`), `variable_name` as `@variable`, builtin command names as `@function.builtin`, and numbers and operators inside `arithmetic_expansion`.

`unit:src/editor/core/cm/interpolation.test.ts` (8 `it` blocks; 6 tests in proofs_editor_highlight_interpolation.rs; the `it.each` tables are loops inside the tests, each language a separate file of one project, every mismatch reported with its language). Expressions are wrapped in a minimal valid program per language (`fun main() { val x = … }`, `object A { val x = … }`, `void main() { var x = …; }`, `class A { void M() { var x = …; } }`) because tree-sitter parses programs, not bare expressions; needles are anchored in the expression. Groovy is left out of every table: languages/groovy.toml has no `tree-sitter`, so Groovy files are not highlighted at all natively (a difference, not built). Not written: "keeps Groovy dotted interpolation in code" (Groovy only, same reason) and "%s uses the same patched mode by name, alias and filename" (CodeMirror's mode registry lookup by name/alias; no native counterpart beyond file detection).

- `separates_delimiters_names_operators_and_numbers`, `keeps_nested_braces_strings_and_comments_inside_expressions`, `preserves_literal_dollars_and_braces`: C# predicted to pass (its query paints the interpolated string `@string`, inner identifiers `@variable`, operators, numbers, `interpolation_brace` as punctuation). EXPECTED FAIL for Kotlin: the kotlin Spec's query (crates/ide-language/queries/kotlin/highlights.scm, 21 lines) only adds declaration roles to the webview's CodeMirror Kotlin mode — no strings, numbers, operators, keywords (except `class_modifier`) or references — and tree-sitter-kotlin-ng ships no query, so natively a Kotlin file is almost uncoloured. EXPECTED FAIL for Scala on the `$i` names (`(interpolation (identifier) @none)` leaves them string-coloured) and hypothesis-fail for Dart (no capture for references inside `template_substitution`; the spec's `{ n -> 2 }` is not Dart syntax either, so its parse is error recovery).
- `handles_kotlin_multiline_raw_templates_and_nested_templates` — EXPECTED FAIL (same Kotlin gap).
- `handles_csharp_verbatim_interpolation_and_doubled_quotes`, `handles_csharp_raw_interpolation_with_multiple_dollar_delimiters` — predicted pass (the raw one is a hypothesis: depends on tree-sitter-c-sharp 0.23.5's raw interpolated strings).
- Fix for Kotlin: a full Kotlin highlights query (keywords, strings with `string_content`, `interpolation`/`${` as punctuation and inner code, numbers, operators, comments) ahead of the existing declaration-role patterns in queries/kotlin/highlights.scm. That query is also the webview's supplement, so the fix must keep the supplement's spans for the webview (e.g. a separate native-only query joined in `native_highlights("kotlin")`, with the Spec's query kept for the webview).

`unit:src/editor/core/cm/syntaxHighlight.test.ts` (2 cases, 2 tests in proofs_editor_highlight_backend.rs):

- `styles_backend_ranges_with_the_active_highlight_style_and_maps_them_through_edits` — EXPECTED FAIL on the mapping: `def` keyword and `:ok` constant are painted from the backend's ranges (pass), but after typing two spaces before them, before the 40 ms re-highlight runs, the words must keep their colours; natively `Editor::follow` (view.rs:702–748) maps selections and marks through the changes but not `self.highlights`, and the paint (view.rs:1844) uses the stale offsets, so `def`'s colour slides two characters left until the new answer. Fix: in `follow`, map each `HighlightSpan` through `changes` with `buffer::map_offset` (start `Assoc::After`, end `Assoc::Before`, dropping empty spans), as `map_marks` does.
- `lets_kotlin_declaration_roles_override_the_stream_lexers_generic_definition_style` — predicted pass: `greet` syn-function and `prefix` syn-param, each one run.
- All four unit rows stay PARTIAL until the failures are fixed; cmake and syntaxHighlight become done once their tests pass (every case written); shellInterpolation done once passing; interpolation stays partial (Groovy cases and the mode-registry case have no native counterpart).

### File 10: app-gpui/src/editor/intelligence/tests/proofs_editor_navigation_modifier.rs (2 tests)

Mod line: hunk 5 above. Uses the module's `open` fixture with the fake language server (`fake_server::FakeServer`), `settle`, `Fixture::{editor, intelligence, requests, close}`.

- `the_plain_hover_yields_while_the_navigation_modifier_is_held` → `module:src/platform/navigationModifier.ts` (PARTIAL). Resting the pointer on `bar` with ⌘ (`Modifiers::secondary_key`) shows no hover after the delay; resting on it without ⌘ shows `bar is a name`. Predicted green.
- `pressing_the_navigation_modifier_closes_an_open_hover` → same row. With the documentation open, a ⌘ press with the pointer still (`simulate_modifiers_change`) must close it. EXPECTED TO FAIL: natively the modifier is followed only from pointer events (`EditorIntelligence::pointer_rests` → `NavigationModifier::pointer_event`, editor/intelligence.rs:921–935); `NavigationModifier::key` and `blur` (editor_lsp/hover.rs:72, 83) have no caller, and the window's `ModifiersChangedEvent` goes only to the shortcut dispatcher (`platform::input::modifiers_changed`, input.rs:249). The webview's `onKey` on keydown/keyup sets the state window-wide. Fix: forward the window's modifier changes to every `EditorIntelligence` of the window (e.g. from `input::modifiers_changed`'s caller in the workbench root view) and call `navigation_modifier.key(..)`, closing the hover when `closes_hover` says so; call `blur()` from a window-activation observer.
- Note: the recipe's location (`editor_lsp::navigation`, `is_navigation_gesture` etc.) was wrong: those live in `editor_lsp::hover` (`is_navigation_gesture`, `NavigationModifier`); `editor_lsp::navigation` holds the go-to flows.
- Not written: the ⌘-hover link underline and its reset on window blur. Natively there is no ⌘-hover link state (no underline is drawn; nothing reads `NavigationModifier::held` but `hover_yields`), so the blur reset is not observable: not built.

## Accessors / hunks the tests depend on

None. The tests read two private fields through module descendancy (legal for child modules of the module that defines the struct): `FindPopup::project_masks` (find/popup_view.rs:284) and `Editor::highlights_version` (editor/view.rs:231). Only the 5 mod-line hunks (10 lines) at the top are needed.

## Rows summary

| Row                                                                                              | Tests                  | After a green run                                                  |
| ------------------------------------------------------------------------------------------------ | ---------------------- | ------------------------------------------------------------------ |
| `ipc:index_file_extensions`                                                                      | masks                  | done                                                               |
| `ipc:languages`                                                                                  | masks                  | done                                                               |
| `module:src/editor/core/copyReference.ts`                                                        | 2 copy-reference tests | partial (direct action missing → 1 expected failure)               |
| `unit:src/editor/core/cm/multiCaret.test.ts`                                                     | 6                      | partial (column drag missing, middle-button case not built)        |
| `module:src/editor/core/clipboardHistory.ts`                                                     | 2                      | partial (Paste from History and outside-editor tracking not built) |
| `unit:src/editor/core/cm/cmake.test.ts`                                                          | 7                      | done once the cmake query is fixed                                 |
| `unit:src/editor/core/cm/shellInterpolation.test.ts`                                             | 4                      | done once the bash query is fixed                                  |
| `unit:src/editor/core/cm/interpolation.test.ts`                                                  | 6                      | partial (Groovy has no grammar; mode-registry case n/a)            |
| `unit:src/editor/core/cm/syntaxHighlight.test.ts`                                                | 2                      | done once highlights map through edits                             |
| `module:src/platform/navigationModifier.ts`                                                      | 2                      | partial (no key/blur tracking, no ⌘-hover link)                    |
| `module:src/editor/core/{actions,backend,index}.ts`                                              | none                   | stay `[~]`; audit above                                            |
| `module:src/platform/textSearch.ts`                                                              | none                   | `[~]` citing the existing find popup test                          |
| `unit:src/editor/core/view/statusItems.test.tsx`, `module:src/editor/core/colorSchemeRuntime.ts` | none                   | list C                                                             |

## Most likely to fail on first run

- By design (behaviour differs from the webview; each listed with its fix above): `the_copy_reference_action_copies_the_qualified_name_without_a_popup`, `restores_alt_drag_column_selection_once_the_caret_gesture_is_removed`, `pressing_the_navigation_modifier_closes_an_open_hover`, `styles_backend_ranges_with_the_active_highlight_style_and_maps_them_through_edits`, most CMake and shell tests, every Kotlin/Scala/Dart part of the interpolation tests.
- Compile risks: the `«»` needle parser in proofs_editor_highlight_support.rs (logic only checked by reading); deref coercion of `&&str` arguments in the interpolation loops; `simulate_modifiers_change` must exist on the `VisualTestContext` of the gpui that gpui-kit 0.9.1 re-exports (it does in gpui-0.2.2 and gpui-fast-0.1.14 test_context.rs:998).
- Environment risks: `proofs_editor_project_masks` waits for the index (`index_file_extensions`) before opening the popup; if the index skips the `.zzq` files the wait still succeeds (it only waits for rs/py/go). `copy_reference_copies_the_qualified_name_of_the_declaration_at_the_caret` relies on `doc_outline` answering from the tree-sitter outline in Light mode (no server) for Java and Rust; if the Java outline names the method differently (`place(int)` is handled by `declared_name`) or nests it under a package symbol, the reference text will show it. `the_masks_list…` expects `*.md` (a recent mask) to stay in the list though no `.md` file exists, as the webview's `recentMasks` keeps it.
- The view tests' `settle` allows about two seconds of wall clock (200 × 10 ms sleeps); `highlighted_files` compiles four or five grammars' highlight queries cold inside it, so a "did not deliver within two seconds" there is a timeout, not a highlighting failure (raise the loop count in a local settle if it happens).
- `leaves_single_quotes_escaped_dollars_comments_and_heredocs_intact` passes vacuously today (nothing is ever painted syn-decorator in shell files); it only bites once the bash query paints delimiters.
- `highlighted_files` opens extra editors with `cx.new` outside the window (as `an_edit_of_several_changes_is_one_undo_step_that_sibling_editors_follow` does); their highlights do not need painting.
