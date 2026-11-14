# proofs8 — `unit:` spec files whose missing cases are test-only

Nothing was compiled or run. Every status below is a hypothesis made by reading until the integrator runs the tests.

## Files written (all new, 554 lines, 23 tests)

| File                                                                                | Lines | Tests |
| ----------------------------------------------------------------------------------- | ----- | ----- |
| `app-gpui/src/editor/view/tests/proofs8_caret_commands.rs`                          | 42    | 1     |
| `app-gpui/src/workbench/tests/proofs8_build.rs`                                     | 49    | 1     |
| `app-gpui/src/workbench/tests/proofs8_file_refactor.rs`                             | 67    | 2     |
| `app-gpui/src/features_a/vim/engine/tests/proofs8_vim.rs`                           | 16    | 1     |
| `app-gpui/src/vcs_diff/comparison/chunk_optimizer/tests/proofs8_chunk_optimizer.rs` | 38    | 2     |
| `app-gpui/src/vcs_more/merge_diff3/tests/proofs8_chunk_optimizer.rs`                | 64    | 2     |
| `app-gpui/src/run/driver/tests/proofs8_chooser_pinned.rs`                           | 84    | 2     |
| `app-gpui/src/run/chooser_view/tests/proofs8_chooser.rs`                            | 44    | 3     |
| `app-gpui/src/workbench/debug/tests/proofs8_recent.rs`                              | 18    | 1     |
| `app-gpui/src/run/store/tests/proofs8_removal.rs`                                   | 74    | 4     |
| `app-gpui/src/features_b/viewers/preview_host/tests/proofs8_preview_only.rs`        | 19    | 1     |
| `app-gpui/src/agents/session_view/tests/proofs8_agent_reply.rs`                     | 23    | 1     |
| `app-gpui/src/everywhere/structure_view/tests/proofs8_file_structure.rs`            | 16    | 1     |

## `mod` lines to add (each inside the file's existing inline `mod tests`)

| File                                                                            | Add                                                   | After the anchor line                                                          |
| ------------------------------------------------------------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------ |
| `app-gpui/src/editor/view.rs` (`mod tests {` at ~2450, NOT `mod editing_tests`) | `mod proofs8_caret_commands;`                         | `    mod proofs_editor_highlight_support;`                                     |
| `app-gpui/src/workbench/mod.rs` (`mod tests {` at ~4075)                        | `mod proofs8_build;` and `mod proofs8_file_refactor;` | `    mod proofs7_quit;`                                                        |
| `app-gpui/src/workbench/debug.rs` (`pub(super) mod tests {` at ~1017)           | `mod proofs8_recent;`                                 | `    mod proofs_debug;`                                                        |
| `app-gpui/src/features_a/vim/engine.rs` (`mod tests {` at ~236)                 | `mod proofs8_vim;`                                    | `    use super::*;` (the one right after `use super::super::rc::parse_vimrc;`) |
| `app-gpui/src/vcs_diff/comparison/chunk_optimizer.rs` (`mod tests {` at ~215)   | `mod proofs8_chunk_optimizer;`                        | `    use super::*;`                                                            |
| `app-gpui/src/run/chooser_view.rs` (`mod tests {` at ~702)                      | `mod proofs8_chooser;`                                | `    use std::cell::RefCell;`                                                  |
| `app-gpui/src/vcs_more/merge_diff3.rs` (`mod tests {` at ~375)                  | `mod proofs8_chunk_optimizer;`                        | `    use super::*;` (right after `use super::test_support::{Random, lines};`)  |
| `app-gpui/src/run/driver.rs` (`mod tests {` at ~1493)                           | `mod proofs8_chooser_pinned;`                         | `    use std::cell::RefCell;`                                                  |
| `app-gpui/src/run/store.rs` (`mod tests {` at ~1976)                            | `mod proofs8_removal;`                                | `    use ide_run::BuildPolicy;`                                                |
| `app-gpui/src/features_b/viewers/preview_host.rs` (`mod tests {` at ~295)       | `mod proofs8_preview_only;`                           | `    use std::rc::Rc;`                                                         |
| `app-gpui/src/agents/session_view.rs` (`mod tests {` at ~2196)                  | `mod proofs8_agent_reply;`                            | `    use std::cell::RefCell;`                                                  |
| `app-gpui/src/everywhere/structure_view.rs` (`mod tests {` at ~1018)            | `mod proofs8_file_structure;`                         | `    use std::time::Duration;`                                                 |

The child files rely on `use super::*;` reaching the parent test module's private helpers and its own `use` imports (as the existing `proofs*` children do).

## How a `.test.tsx` case was counted

Strict, as the assignment says: a case proven only on a model (no view, no window) counts as missing for a `.test.tsx` spec, even when the spec case itself renders nothing. The **proposed entries use the strict count**.

Several such cases call only a store or a pure function, so a "spec renders nothing" reading would count them. The precedent for it is weak: `parity.json` still has `Grid.test.tsx` partial. Each table marks these cases **model only**, and each row gives the count under that reading as a labelled alternative.

---

## `unit:src/editor/core/cm/caretCommands.test.ts` → 6/6, done (`.ts`)

| Case                                                                        | Rust test                                                                                                     | Status                          |
| --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| breaks the line at each caret and leaves the caret before the break         | `editor_commands::caret_commands::tests::breaks_the_line_at_each_caret_and_leaves_the_caret_before_the_break` | existing                        |
| goes to just inside the enclosing braces                                    | `…::goes_to_just_inside_the_enclosing_braces`                                                                 | existing                        |
| jumps across a brace at the caret to its match, and outward from inside one | `…::jumps_across_a_brace_at_the_caret_to_its_match_and_outward_from_inside_one`                               | existing                        |
| skips braces in strings and comments                                        | `…::skips_braces_in_strings_and_comments`                                                                     | existing                        |
| has nowhere to go outside any block                                         | `…::has_nowhere_to_go_outside_any_block`                                                                      | existing                        |
| drops CodeMirror defaults that IDEA uses for other actions                  | `editor::view::tests::proofs8_caret_commands::drops_codemirror_defaults_that_idea_uses_for_other_actions`     | NEW, editor view with real keys |

What the new test proves: with no keymap, these keys leave the text and carets unchanged: primary+Enter, Ctrl+M, Ctrl+L, primary+Alt+↑/↓, primary+[ and primary+]. The editor's own key handling (`Editor::key_down`, the native base keymap) leaves them to the IDE keymap. Primary+Shift+Z redoes.

Limit: the redo is Ctrl+Shift+Z only on a non-macOS host (`PRIMARY` is cfg-selected). On macOS the test proves ⌘⇧Z. The spec's Windows half is proven only where the tests run on Windows or Linux.

Proposed entry: `{"status": "done", "proof": "`editor_commands::caret_commands::tests`, `editor::view::tests::proofs8_caret_commands::drops_codemirror_defaults_that_idea_uses_for_other_actions`; 6 of 6 cases (the base keymap case on the editor view with real keys; Ctrl+Shift+Z redo is asserted as primary+Shift+Z, Ctrl on Windows and Linux hosts)"}`

## `unit:src/features/build/build.test.tsx` → strict 4/13, partial (13/13 if model proof of non-rendering cases is accepted)

| Case                                                                                  | Rust test                                                                                                      | Status                                                                                                                                                                |
| ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| groups messages by file, errors first, build messages last                            | `build_window::model::tests::groups_messages_by_file_errors_first_build_messages_last`                         | model only                                                                                                                                                            |
| formats IDEA's finish summaries                                                       | `…model::tests::formats_ideas_finish_summaries`                                                                | model only                                                                                                                                                            |
| splits console output into capped lines                                               | `…model::tests::splits_console_output_into_capped_lines`                                                       | model only                                                                                                                                                            |
| shows the CMake hammer and builds the selected target through the shared action       | `build_window::view::tests::shows_the_hammer_for_cmake_and_builds_through_the_shared_action`                   | view                                                                                                                                                                  |
| registers the Build menu actions, shortcuts and an on-use bottom tool window          | `workbench::tests::proofs8_build::registers_the_build_menu_actions_shortcuts_and_an_on_use_bottom_tool_window` | NEW, project window                                                                                                                                                   |
| streams a failing build into the tree and console, then notifies and clears next time | `build_window::view::tests::streams_a_failing_build_into_the_tree_and_console` + model test of the same name   | view                                                                                                                                                                  |
| opens the Build window without focus when %s starts                                   | `…model::tests::opens_the_build_window_without_focus_when_a_build_starts`                                      | model only (the spec checks the `show` mock, renders nothing)                                                                                                         |
| keeps before-launch builds hidden until a failure                                     | `…model::tests::keeps_before_launch_builds_hidden_until_a_failure`                                             | model only (renders nothing)                                                                                                                                          |
| awaits %s only when requested and keeps its successful build hidden                   | `…model::tests::awaits_a_build_only_when_requested_and_keeps_its_successful_build_hidden`                      | model only (renders nothing)                                                                                                                                          |
| keeps ordinary %s calls nonblocking                                                   | `…model::tests::keeps_ordinary_build_module_and_artifacts_calls_nonblocking`                                   | model only (renders nothing)                                                                                                                                          |
| starts a build without a workbench                                                    | `…model::tests::starts_a_build_without_a_workbench`                                                            | model only (renders nothing)                                                                                                                                          |
| shows a message on single click and navigates on F4                                   | `build_window::view::tests::shows_a_message_on_single_click_and_navigates_on_f4`                               | view                                                                                                                                                                  |
| builds the module of the given file and can be stopped                                | `…model::tests::builds_the_module_of_the_given_file_and_can_be_stopped`                                        | model only (renders nothing); `workbench::tests::a_cargo_build_lists_its_error_succeeds_once_fixed_and_ends_cancelled_when_stopped` stops a project build in a window |

What the new test proves:

- `build.buildProject`, `build.buildModule`, `build.rebuildProject` and `build.buildArtifacts` are each in the registry and enabled in a project window's context.
- `intellij-macos` binds `Meta+F9` and `Meta+Shift+F9`.
- The layout's `build` window is `Anchor::Bottom`, `Stripe::OnUse`, icon `toolwindow-build`, with no stripe button before use.
- The Project view's group rows (`ContextMenu::ProjectView`) for a folder hold `build.buildModule` with the folder's path as its argument.

Proposed entry (strict): `{"status": "partial", "proof": "`build_window::view::tests`, `workbench::tests::proofs8_build::registers_the_build_menu_actions_shortcuts_and_an_on_use_bottom_tool_window`, `build_window::model::tests`; 4 of 13 cases on the Build window or a project window; groups, finish summaries, capped console lines, opening without focus, hidden before-launch builds, awaited builds, nonblocking calls, a build without a workbench and the module build that is stopped are proven only on `build_window::model`"}`. Alternative, if model proof of the cases that render nothing is accepted: done, 13 of 13.

## `unit:src/features/project-tree/fileRefactor.test.tsx` → strict 3/6 once the fix lands, partial (6/6 with model proof of the pure cases)

| Case                                                                         | Rust test                                                                                                                                                                                                        | Status                                           |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| retains version guards and both sources' preview fingerprints                | `project_view::file_refactor::tests::retains_version_guards_and_both_sources_preview_fingerprints`                                                                                                               | model only (pure function)                       |
| merges text occurrences that no server edit covers and drops file operations | `…::merges_text_occurrences_that_no_server_edit_covers_and_drops_file_operations`                                                                                                                                | model only (pure)                                |
| knows when other files change and where paths move                           | `…::knows_when_other_files_change_and_where_paths_move`                                                                                                                                                          | model only (pure)                                |
| previews edits in other files, applies the move and offers Undo              | `project_view::file_dialogs::tests::previews_edits_in_other_files_applies_the_move_and_offers_undo` (preview view) + `workbench::tests::rename_renames_the_file_its_reference_and_its_open_editor` (apply, Undo) | existing                                         |
| excluded files are left out and Cancel keeps everything                      | `project_view::file_dialogs::tests::excluded_files_are_left_out_and_cancel_keeps_everything`                                                                                                                     | existing (preview view)                          |
| skips the search without options and stops when cancelled                    | `workbench::tests::proofs8_file_refactor::skips_the_search_without_options` + `…::stops_when_cancelled_before_the_search_answers`                                                                                | NEW, project window (two tests for one sentence) |

What the new tests prove:

- **skip:** both checkboxes are unticked by clicking `file-dialog-references` and `file-dialog-text`, then `file-dialog-ok` is clicked. The file is renamed with no preview, `README.md` keeps `old.txt`, and "Rename 'old.txt' to 'new.txt'" is notified.
- **stop:** `enter escape` is pressed in one go. Once the refactoring releases its dialog (WeakEntity), the file keeps its name, `README.md` is unchanged and no preview opened.

**Expected failure: `stops_when_cancelled_before_the_search_answers`.**

- Why: `run_refactoring` (`workbench/project_files.rs` ~441) stores the `SearchStop` in `files.reference_search` only once its task runs, after `save_all`.
- So the Escape handled in `file_dialog_event` (`FileDialogEvent::Cancelled`, ~1227) finds `reference_search == None` and stops nothing.
- The task then runs unstopped and opens the preview, which trips the `!preview_open` assert.
- The spec case (an aborted signal ⇒ "cancelled") requires the stop to hold from the moment the refactoring is asked for.

Fix (contained, `app-gpui/src/workbench/project_files.rs`):

```diff
@@ async fn run_refactoring(
     label: RefactorLabel,
     dialog: Option<AnyView>,
+    stop: Arc<SearchStop>,
 ) -> anyhow::Result<RefactorStatus> {
@@
-    let stop = Arc::new(SearchStop::default());
-    this.update(cx, |workbench, _| workbench.files.reference_search = Some(stop.clone()))?;
-    let collected = if searching {
+    let collected = if stop.is_stopped() {
+        None
+    } else if searching {
         collect_edits(&this, cx, &moves, options, &stop).await?
     } else {
@@ impl Workbench {
+    /// Starts the reference search of a refactoring: from now on its dialog's Cancel stops it.
+    fn start_reference_search(&mut self) -> Arc<SearchStop> {
+        let stop = Arc::new(SearchStop::default());
+        self.files.reference_search = Some(stop.clone());
+        stop
+    }
@@ Request::Refactor { moves, label } => {
                 let options = load_refactor_options(stored);
+                let stop = self.start_reference_search();
                 cx.spawn_in(window, async move |this, cx| {
-                    let status = run_refactoring(this.clone(), cx, moves, options, label, None).await;
+                    let status = run_refactoring(this.clone(), cx, moves, options, label, None, stop).await;
@@ FileDialogEvent::Refactor { label, moves, options } => {
                 let view = AnyView::from(dialog.clone());
+                let stop = self.start_reference_search();
                 cx.spawn_in(window, async move |this, cx| {
-                    let status =
-                        run_refactoring(this.clone(), cx, moves.clone(), options, label, Some(view.clone())).await;
+                    let status = run_refactoring(this.clone(), cx, moves.clone(), options, label, Some(view.clone()), stop)
+                        .await;
```

The `start_reference_search` method goes inside an existing `impl Workbench` block of the same file (e.g. next to `start_flow`). Line 449 already clears `reference_search` after collecting.

Proposed entry (strict, after the hunk): `{"status": "partial", "proof": "`project_view::file_dialogs::tests`, `workbench::tests::rename_renames_the_file_its_reference_and_its_open_editor`, `workbench::tests::proofs8_file_refactor::{skips_the_search_without_options, stops_when_cancelled_before_the_search_answers}`, `project_view::file_refactor::tests`; 3 of 6 cases through the preview or a project window; the edit merging, version guards and moved paths are proven only on `project_view::file_refactor`"}`. Before the hunk: 2 of 6, with "stops when cancelled" failing. Alternative with model proof of the pure cases: done, 6 of 6 (after the hunk).

## `unit:src/features/vim/__tests__/vim.test.ts` → 15/17 + 1 half, partial (`.ts`)

The 15 existing cases are those the proof cites, matched by name in `features_a::vim::{mode,engine,ime}::tests`: toggle, follows the setting, `:action`, `:w/:q/:wq`, rc mappings, insert-mode mappings, leader shadowing a built-in, built-in operators, consistent across reloads, restores the built-in key, the four IME cases, publishes the mode.

| Case                                   | Rust test                                                                             | Status                                                                                                                                                                                                                                                                                                                                                         |
| -------------------------------------- | ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| inserts escaped key notation literally | `features_a::vim::engine::tests::proofs8_vim::inserts_escaped_key_notation_literally` | NEW, half: the mapping resolves to the keys `<lt>`, `E`, `s`, `c`, `>`, `\|`, `\` with no Escape. That they are typed as `<Esc>\|\` and that Insert mode stays is NOT proven: no native code types Vim keys into the editor (same product gap as the next case).                                                                                               |
| matches browser key events for %s      | —                                                                                     | missing, product. There is no GPUI keystroke → Vim key conversion (the spec's `vimKeyFromEvent`). The native editor feeds no keys to a Vim engine at all: `RcKeymap` is used only by `workbench/status_items.rs`. It is not "no native counterpart": the native counterpart would be a `Keystroke` → `<C-f>`/`<S-Tab>`/`<D-x>` mapping, and it does not exist. |

Proposed entry: `{"status": "partial", "proof": "`features_a::vim::mode::tests`, `features_a::vim::engine::tests`, `features_a::vim::ime::tests`, `features_a::vim::engine::tests::proofs8_vim::inserts_escaped_key_notation_literally`; 15 of 17 cases; \"inserts escaped key notation literally\" is proven only as far as the keys the mapping resolves to, and \"matches browser key events\" not at all: the editor feeds no keystrokes to a Vim engine"}`

## `unit:src/features/vcs/__tests__/chunkOptimizer.test.ts` → 4/4, done (`.ts`)

| Case                                                                            | Rust test                                                                                                                                           | Status                                                                                                                                                                         |
| ------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| moves a block that starts in the middle of a copy to where the copy starts      | `vcs_diff::comparison::chunk_optimizer::tests::proofs8_chunk_optimizer::moves_a_block_that_starts_in_the_middle_of_a_copy_to_where_the_copy_starts` | NEW, on the optimizer the diff and the merge use: `Range(2,2,2,5)` over 6 and 9 lines becomes `Range(0,0,0,3)`                                                                 |
| leaves a change that already starts and ends at a blank line alone              | `…proofs8_chunk_optimizer::leaves_a_change_that_already_starts_and_ends_at_a_blank_line_alone`                                                      | NEW, same optimizer: `Range(2,2,2,4)` stays                                                                                                                                    |
| only rearranges: the unchanged stretches stay equal, ordered, and as many lines | `vcs_more::merge_diff3::tests::proofs8_chunk_optimizer::only_rearranges_the_unchanged_stretches_stay_equal_ordered_and_as_many_lines`               | NEW: 500 seeded rounds (the spec's LCG, seed 99, same vocabulary) through `merge_diff3::against` (`diff_keys` + `vcs_diff::comparison::chunk_optimizer::optimize_line_chunks`) |
| puts an added function between its neighbours, not across their braces          | `vcs_more::merge_diff3::tests::proofs8_chunk_optimizer::puts_an_added_function_between_its_neighbours_not_across_their_braces`                      | NEW, through `merge_changes`                                                                                                                                                   |

**Finding.** The tests the current proof cites (`vcs::conflicts::tests::moves_a_block…`, `…leaves_a_change…`) run `vcs::conflicts::optimize_line_chunks`. No product code calls that function: grep finds no caller outside `vcs/conflicts.rs`. The diff (`vcs_diff::comparison::by_line`) and the merge (`vcs_more::merge_diff3::against`) use `vcs_diff::comparison::chunk_optimizer::optimize_line_chunks`. The two first cases are now written against that one.

The weights in the new tests are characters other than white space, as in the spec, not `line.len()` as the module's own `infos` helper uses. If the reached optimizer answers differently from the spec on either case, the test fails. That is a real difference between the two optimizers, and the copy in `vcs::conflicts` is then the one that matches the spec.

`vcs::conflicts::optimize_line_chunks` and its two tests look like dead code to delete (product decision, not done).

Proposed entry: `{"status": "done", "proof": "`vcs_diff::comparison::chunk_optimizer::tests`(with`proofs8_chunk_optimizer`), `vcs_more::merge_diff3::tests::proofs8_chunk_optimizer`; 4 of 4 cases, all through the optimizer the diff and the three-way merge use"}`

## `unit:src/features/run/chooserPinned.test.tsx` → strict 3/5, partial

| Case                                                                          | Rust test                                                                                                                                                                                                                                                                                                                                                                                 | Status                                                                                                                                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| lists pinned configurations first, in pin order, and not again in their group | `run::chooser_view::tests::lists_pinned_configurations_first_in_pin_order_and_not_again_in_their_group`                                                                                                                                                                                                                                                                                   | existing, view                                                                                                                                                                           |
| keeps pins per project across reloads                                         | `run::driver::tests::proofs8_chooser_pinned::keeps_pins_per_project_across_reloads`                                                                                                                                                                                                                                                                                                       | NEW, but driver + backend + UI storage, no view or window: **model only** under the strict rule. A second and a third driver read the stored pins back; another opened project has none. |
| pins and unpins from the row's button                                         | `run::chooser_view::tests::pins_and_unpins_from_the_rows_button`                                                                                                                                                                                                                                                                                                                          | existing, view                                                                                                                                                                           |
| lists the configurations started last after the pinned ones, newest first     | `run::chooser_view::tests::proofs8_chooser::lists_the_configurations_started_last_after_the_pinned_ones_newest_first`                                                                                                                                                                                                                                                                     | NEW, view (also `run::config_chooser::tests::lists_pinned_then_recent_then_groups_by_kind` on the model)                                                                                 |
| marks a configuration recent when a run or debug session starts               | run half: `run::driver::tests::proofs8_chooser_pinned::marks_a_configuration_recent_when_a_run_session_starts` (two real `/bin/echo` runs, stored, read back; driver only, **model only** under the strict rule). Debug half: `workbench::debug::tests::proofs8_recent::marks_a_configuration_recent_when_a_debug_session_starts` (Run › Debug in a project window with the fake adapter) | NEW; the debug half is an **expected failure**                                                                                                                                           |

**Expected failure (by reading): `marks_a_configuration_recent_when_a_debug_session_starts`.**

- `run.debug` → `Workbench::debug_selected` → `debug_config` (`workbench/debug.rs` ~445) → `run_debug_action("debug.startConfig")` → `debugger/actions.rs` ~604 `start_config`.
- That path never reaches the run store's start flow, the only place `StartEffect::RecordRun` is pushed (`run/store.rs` ~1706).
- `grep recent` finds nothing in `debugger/` or `workbench/debug.rs`, so the recent list stays empty.
- The spec demands it: `trackRecentConfigurations` watches the debug sessions.

Fix (contained):

```diff
--- app-gpui/src/run/driver.rs (in `impl RunDriver`, next to `toggle_pinned`)
+    /// A configuration was started under the debugger: it leads the chooser's recent configurations.
+    pub fn mark_recent(&mut self, id: &str, cx: &mut Context<Self>) {
+        let write = self.store.update(cx, |store, _| store.mark_recent(id));
+        Self::persist(write, cx);
+    }
--- app-gpui/src/workbench/debug.rs (Workbench::debug_config)
                 let Some(config) = run.update(cx, |run, cx| run.config_from_arg(arg.as_ref(), cx)) else { return };
+                run.update(cx, |run, cx| run.mark_recent(&config.id, cx));
                 let mut start = json!({ "config": serde_json::to_value(&config).unwrap_or(Value::Null) });
```

Debug sessions started without `debug_config` (`run.debugContext`, the gutter's Debug) would still not be recorded. I did not read those paths.

`MAX_RECENT` truncation is proven on the store (`run::store::tests::pinned_and_recent_ids_are_stored_per_project`).

Proposed entry (strict, after the fix): `{"status": "partial", "proof": "`run::chooser_view::tests`(with`proofs8_chooser`), `workbench::debug::tests::proofs8_recent::marks_a_configuration_recent_when_a_debug_session_starts`, `run::driver::tests::proofs8_chooser_pinned`; 3 of 5 cases on the chooser; pins kept per project across reloads and a run marking its configuration recent are proven on the run driver without a window, a debug session marking it recent in a project window"}`. Alternative with model proof of the non-rendering cases: done, 5 of 5, once the debug fix lands.

## `unit:src/features/run/removal.test.tsx` → strict 9/11, partial

| Case                                                                          | Rust test                                                                                                                                                                                                                                                                                          | Status              |
| ----------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- |
| enables minus for detections and stays removed after Apply and backend reload | `run::edit_configs_view::tests::enables_minus_for_detections_and_stays_removed_after_apply_and_backend_reload`                                                                                                                                                                                     | existing, view      |
| handles Delete only inside the tree                                           | `…::handles_delete_only_inside_the_tree`                                                                                                                                                                                                                                                           | existing, view      |
| keeps the override label and removes overrides without suppression            | `…::keeps_the_override_label_and_removes_overrides_without_suppression`                                                                                                                                                                                                                            | existing, view      |
| chooser deletion suppresses detected %s configurations                        | `run::chooser_view::tests::proofs8_chooser::chooser_deletion_suppresses_detected_configurations` (view: each of the 8 kinds selected, Delete pressed, the Delete event, the save `([], [id])`) + `run::store::tests::proofs8_removal::chooser_deletion_suppresses_detected_configurations` (store) | NEW, view           |
| temporary deletion does not persist suppression                               | `run::chooser_view::tests::proofs8_chooser::temporary_deletion_does_not_persist_suppression` (view: Delete, the row is gone, no temporary left) + `run::store::tests::proofs8_removal::temporary_deletion_does_not_persist_suppression` (store: nothing saved)                                     | NEW, view           |
| a moved file drops its stale temporary context configuration                  | `run::store::tests::proofs8_removal::a_moved_file_drops_its_stale_temporary_context_configuration`                                                                                                                                                                                                 | NEW, **model only** |
| keeps other temporary and non-temporary configurations on removal             | `run::store::tests::proofs8_removal::keeps_other_temporary_and_non_temporary_configurations_on_removal`                                                                                                                                                                                            | NEW, **model only** |
| chooser popup Delete removes the selected detection                           | `run::chooser_view::tests::chooser_popup_delete_removes_the_selected_detection`                                                                                                                                                                                                                    | existing, view      |
| Cancel discards a pending detected removal                                    | `run::edit_configs_view::tests::cancel_discards_a_pending_detected_removal`                                                                                                                                                                                                                        | existing, view      |
| removes a temporary configuration from Edit Configurations for good           | `…::removes_a_temporary_configuration_from_edit_configurations_for_good`                                                                                                                                                                                                                           | existing, view      |
| keeps unedited temporary configurations temporary and saves edited ones       | `…::keeps_unedited_temporary_configurations_temporary_and_saves_edited_ones`                                                                                                                                                                                                                       | existing, view      |

**Difference.** For a detected configuration, the spec's `deleteConfig` ends with `configs` empty. Natively, `RunStore::delete_config` returns the save `(user, [id])`, and the list empties only once `RunDriver::delete_config` has saved and listed again (`run::driver::tests::selecting_pinning_and_deleting_are_stored_and_saved` proves the relisting for a saved configuration). The new tests assert the save, not the emptied list.

The two model-only cases would need a project window: move a file whose gutter run made a context configuration, then read the run chooser. Not written.

Proposed entry (strict): `{"status": "partial", "proof": "`run::edit_configs_view::tests`, `run::chooser_view::tests`(with`proofs8_chooser`), `run::store::tests::proofs8_removal`; 9 of 11 cases on Edit Configurations or the chooser; a moved file dropping its context configuration and keeping the others are proven only on `run::store`"}`. Alternative with model proof of the non-rendering cases: done, 11 of 11.

## `unit:src/features/viewers/markdown/previewOnly.test.tsx` → 5/5, done

| Case                                                                              | Rust test                                                                                                                    | Status                                                                                                                                                                 |
| --------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| scrolls to where the hidden editor's caret is sent, again on the same place       | `features_b::viewers::preview_host::tests::scrolls_to_where_the_hidden_editors_caret_is_sent_again_on_the_same_place`        | view                                                                                                                                                                   |
| reports the line on screen as the user scrolls, not after navigating              | `…::reports_the_line_on_screen_as_the_user_scrolls_not_after_navigating`                                                     | view                                                                                                                                                                   |
| reports the restored line when shown again after a remount                        | `features_b::viewers::preview_host::tests::proofs8_preview_only::reports_the_restored_line_when_shown_again_after_a_remount` | NEW, view. It switches Editor → set `preview_scrolled(top_of(80) + 5)` → Preview; asserts the scroll offset, `shown_line() == Some(80)` and the last `ShownLine` event |
| leaves the split preview to the editor's scroll sync                              | `…::leaves_the_split_preview_to_the_editors_scroll_sync`                                                                     | view                                                                                                                                                                   |
| stops standing in for the editor when the editor comes back or the preview closes | `…::stops_standing_in_for_the_editor_when_the_editor_comes_back_or_the_preview_closes`                                       | view                                                                                                                                                                   |

Proposed entry: `{"status": "done", "proof": "`features_b::viewers::preview_host::tests`(with`proofs8_preview_only::reports_the_restored_line_when_shown_again_after_a_remount`), `features_b::viewers::preview::tests`; 5 of 5 cases on the view"}`

## `unit:src/features/agents/AgentReply.test.tsx` → 3/3 + 1 no native counterpart

| Case                                                                     | Rust test                                                                                                                    | Status                                                                                                                                                                                   |
| ------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| copies the exact Markdown source and briefly confirms success            | `agents::session_view::tests::copies_the_exact_markdown_source_and_briefly_confirms_success`                                 | view                                                                                                                                                                                     |
| copies the latest streamed text and clears feedback when content changes | `agents::session_view::tests::proofs8_agent_reply::copies_the_latest_streamed_text_and_clears_feedback_when_content_changes` | NEW, view: click copy, the reply grows in the store, the button says Copy Markdown again, a second click copies the whole reply                                                          |
| reports a clipboard failure without claiming success and allows retry    | `agents::reply_actions::tests::reports_a_clipboard_failure_without_claiming_success_and_allows_retry` (model)                | no native counterpart: GPUI's `write_to_clipboard` cannot fail (the spec rejects `navigator.clipboard.writeText`, a browser permission); `session_view.rs` ~1106 always reports `Ok(())` |
| does not offer copying an empty reply (%j)                               | `agents::session_view::tests::does_not_offer_copying_an_empty_reply`                                                         | view                                                                                                                                                                                     |

Proposed entry: `{"status": "done", "proof": "`agents::session_view::tests`(with`proofs8_agent_reply`), `agents::reply_actions::tests`; 3 of 3 cases with a native counterpart on the conversation view; \"reports a clipboard failure\" has no native counterpart (the native clipboard write cannot fail)"}`

## `unit:src/features/search/__tests__/fileStructure.test.tsx` → 4/4, done (numbers were wrong)

| Case                                                                          | Rust test                                                                                                                                                                                      | Status                                                                                                                                                                                                                                                                                |
| ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| shows the outline as an expanded tree; ←/→ collapse and expand                | `everywhere::structure_window::tests::shows_the_outline_as_an_expanded_tree_left_and_right_collapse_and_expand`, `everywhere::structure_view::tests::the_arrows_walk_fold_and_unfold_the_tree` | existing                                                                                                                                                                                                                                                                              |
| narrows down on typing keeping ancestors, or only highlights when that is off | `everywhere::structure_view::tests::proofs8_file_structure::narrows_down_on_typing_keeping_ancestors_or_only_highlights_when_that_is_off`                                                      | NEW, popup view. "creat" leaves `OrderController`, `create`; a click on `file-structure-narrow` turns narrowing off and shows all five rows with one match. The existing `without_narrowing_typing_highlights_and_the_arrows_go_from_match_to_match` turned it off on the model only. |
| Esc clears the filter first, then closes                                      | `everywhere::structure_view::tests::typing_narrows_the_tree_and_escape_clears_the_filter_before_it_dismisses`                                                                                  | existing (missed by name: the proof cites only `structure_window::tests`)                                                                                                                                                                                                             |
| offers Show inherited members only when the server knows type hierarchies     | `everywhere::structure_window::tests::offers_show_inherited_members_only_when_the_server_knows_type_hierarchies`                                                                               | existing                                                                                                                                                                                                                                                                              |

Proposed entry: `{"status": "done", "proof": "`everywhere::structure_window::tests`, `everywhere::structure_view::tests`(with`proofs8_file_structure`); 4 of 4 cases on the File Structure popup"}`

## `unit:src/features/todo/todo.test.ts` → 12/13, no test written

The 12 cases are matched by name in `features_a::todo::tests`: keeps, drops, falls back, builds the regex, keeps comment matches, includes non-comment matches, finds items, groups, filters, layers live results, maps scopes, keeps only change-list files.

"decorates TODO comments only" is missing, product: the native editor draws no TODO decoration. Nothing under `editor/` reads the TODO patterns. The colour scheme has a `todo` attribute (`theme/color_scheme.rs:186`) that nothing applies to the text. It needs a view, not a test.

Index numbers were wrong: 10 of 13 covered and `missing_test_only = 3` should be 12 of 13 and `missing_needs_view = 1`.

Proposed entry: `{"status": "partial", "proof": "`features_a::todo::tests`; 12 of 13 cases: \"decorates TODO comments only\" needs the editor to highlight TODO comments, which it does not"}`

---

## Accessors / product hunks the tests depend on

- No new accessor. Every accessor used exists: `Workbench::{tool_windows, group_rows, file_dialog, run_driver}`, `ToolWindowLayout::{window, has_stripe_button}`, `FileDialog::{checkboxes, name_field}`, `RunDriver::{load, when_loaded, toggle_pinned, select_config, launch_selected, store}`, `RunStore::{delete_config, can_delete_config, drop_stale_context_configs, adopt_temporary}`, `FileStructure::{options, lines, matched}`, `SessionView::buttons`, `LiveSources::preview_scrolled`.
- Product fix for `stops_when_cancelled_before_the_search_answers`: the `project_files.rs` hunk above.
- Product fix for `marks_a_configuration_recent_when_a_debug_session_starts`: the `run/driver.rs` + `workbench/debug.rs` hunk in the chooserPinned section.
- New tests also use `ConfigChooser::{lines, selected}`, `RunStore::{pinned, recent, temporary}` (pub fields); all exist.

## Tests most likely to fail first

1. `workbench::tests::proofs8_file_refactor::stops_when_cancelled_before_the_search_answers`: an expected failure (the stop is registered after the Escape), fixed by the hunk.
2. `workbench::debug::tests::proofs8_recent::marks_a_configuration_recent_when_a_debug_session_starts`: an expected failure by reading (the debug launch never calls `mark_recent`), fixed by the chooserPinned hunk.
3. `vcs_diff::comparison::chunk_optimizer::tests::proofs8_chunk_optimizer::*`: the reached optimizer (IntelliJ's `ChunkOptimizer` port) may place the first case's block differently from the spec's `[[0,0,0,3]]`. If so, it is a real difference between the diff's optimizer and the spec.
4. `run::chooser_view::tests::proofs8_chooser::chooser_deletion_suppresses_detected_configurations`: `move_to` matches the selected row by the configuration's name (the kind here). If the chooser shows a type's own title instead, adjust the names.
5. `features_b::viewers::preview_host::tests::proofs8_preview_only::reports_the_restored_line_when_shown_again_after_a_remount`: `PreviewScroll::mount` computes the shown line from the layout at mount time. If the new preview's blocks are not laid out yet, it reports `Some(0)` first and the last event may differ. The `shown()` assert is the meaningful one.
6. `editor::view::tests::proofs8_caret_commands::drops_codemirror_defaults_that_idea_uses_for_other_actions`: the caret after redo (`3`) assumes redo restores the caret after the typed `x`; `ctrl-m`/`ctrl-l` on macOS assume `simulate_keystrokes` sends no text input for a Control chord.
7. `workbench::tests::proofs8_file_refactor::skips_the_search_without_options`: the checkbox clicks go through `click_on` on the dialog layer. If the dialog's checkboxes are not hit-tested at their debug bounds, the boxes stay ticked; the second assert says so.
8. `agents::session_view::tests::proofs8_agent_reply::…`: assumes the conversation re-renders the reply from the store's new entries after `cx.notify()` (the view observes the store, `session_view.rs:641`).

## Behaviours that differ from the spec

- A refactoring cancelled before its task first runs is not stopped (fileRefactor, fix above).
- A debug session start does not mark its configuration recent (chooserPinned, fix above).
- The native editor has no TODO comment decoration (todo).
- The native editor has no Vim key input at all: `RcKeymap` resolves mappings, but no keystroke reaches it (vim, "matches browser key events").
- Detected-configuration deletion empties the list only after the save and relist (removal; not a defect, the order of steps differs).

## Spec files whose recorded numbers were wrong

- `fileStructure.test.tsx`: recorded 2 of 4. Read, 3 of 4 were already covered: "Esc clears the filter first" is `structure_view::tests::typing_narrows_the_tree_and_escape_clears_the_filter_before_it_dismisses`, and the narrowing half is in `structure_view`. The proof cites only `structure_window`.
- `todo.test.ts`: recorded 10 of 13 with 3 test-only. Read, it is 12 of 13 with 1 needing a view.
- `build.test.tsx`: recorded 12 of 13 with 1 test-only. By name 12 of 13 is right. Under the strict `.test.tsx` rule only 3 were on a view (cmake, streams, F4): 9 were missing, not 1.
- `vim.test.ts`: the 2 missing were recorded as test-only. One ("matches browser key events") needs product.
- `AgentReply.test.tsx`: recorded 2 test-only. One of them (clipboard failure) has no native counterpart.
- `chunkOptimizer.test.ts`: the two "covered" cases were proven on `vcs::conflicts::optimize_line_chunks`, which no product code calls (now also written against the reached optimizer).
- `chooserPinned.test.tsx`: "lists the configurations started last…" was covered on the model (`run::config_chooser::tests::lists_pinned_then_recent_then_groups_by_kind`), and one of the three "test-only" cases (a debug start marking recent) needs product.
- `removal.test.tsx`: two of the four "test-only" cases (moved file) need a project-window test through file moves, which is more than a view test.
- `fileRefactor.test.tsx`: the "test-only" case needs a product fix (cancel before the search registers).

## Skipped (covered by other drafts' notes)

- `Grid.test.tsx`: proofs4 (stale gap, done).
- `executionState.test.ts`: proofs7 (done with an existing test).
- `languages.test.ts`: proofs7 (needs product).
- `windows.test.ts`: proofs7 / proofs_workbench (needs a shared helper).
- `keymap.test.ts`: proofs7.

## Where I stopped

All 11 candidate spec files with `missing_test_only` ≤ 4 that no other draft covers were handled.

Under the strict rule these become done: caretCommands, chunkOptimizer, previewOnly, AgentReply, fileStructure. These stay partial:

- build 4/13
- fileRefactor 3/6 after its fix
- chooserPinned 3/5 after its fix
- removal 9/11
- vim 15/17
- todo 12/13

In order: caretCommands, build, fileRefactor, previewOnly, AgentReply, fileStructure, chunkOptimizer, vim, todo (no test), chooserPinned, removal.

I stopped before the files with 5 or more missing cases, none of which was started:

- `diffDialog.test.tsx` (5)
- `tasks.test.tsx` (6)
- `list/list.test.tsx` (7)
- `results-view.test.tsx` (8)
- `AgentsView.test.tsx` (10)
- `messageEditing.test.tsx` (11)
- `diffToolbar.test.tsx` (11)
- `movedCode.test.tsx` (12)
- `Popup.test.tsx` (12)
- `Markdown.test.tsx` (15)
- `sessionState.test.tsx` (18)
- `unread.test.tsx` (18)
