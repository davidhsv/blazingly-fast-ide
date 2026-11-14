# proofs11 windows: tests drafted without a compiler

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. All paths below are relative to `app-gpui/src/`.

## `mod` lines to add

| File written                                                    | `mod` line                      | Where (anchor)                                                                                                                                                                                                        |
| --------------------------------------------------------------- | ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workbench/tests/proofs11_vcs_windows.rs`                       | `mod proofs11_vcs_windows;`     | `workbench/mod.rs`, inside `mod tests`, after `    mod proofs9_run_chooser;` (alphabetical; anywhere in the block works)                                                                                              |
| `workbench/shelf/tests/proofs11_shelf.rs`                       | `mod proofs11_shelf;`           | `workbench/shelf.rs`, inside the second `mod tests {` (line ~2367, the one whose first line is `    use super::super::tests::{`), e.g. after `    use serde_json::json;`. The folder `workbench/shelf/tests/` is new. |
| `workbench/vcs_operations/tests/proofs11_worktrees.rs`          | `mod proofs11_worktrees;`       | `workbench/vcs_operations.rs`, inside `mod tests`, next to `    mod proofs_update;`                                                                                                                                   |
| `workbench/services_actions/tests/proofs11_docker.rs`           | `mod proofs11_docker;`          | `workbench/services_actions.rs`, inside `mod tests`, next to `    mod proofs7_move;`                                                                                                                                  |
| `workbench/debug/tests/proofs11_hotswap.rs`                     | `mod proofs11_hotswap;`         | `workbench/debug.rs`, inside `pub(super) mod tests`, next to `    mod proofs_debug;`                                                                                                                                  |
| `workbench/editor_core/tests/proofs11_read_only.rs`             | `mod proofs11_read_only;`       | `workbench/editor_core.rs`, inside `mod tests`, next to `    mod proofs7_read_only;`                                                                                                                                  |
| `workbench/plugin_contributions/tests/proofs11_plugin_debug.rs` | `mod proofs11_plugin_debug;`    | `workbench/plugin_contributions.rs`, inside `mod tests`, next to `    mod untrusted_windows;`                                                                                                                         |
| `workbench/project_view_actions/tests/proofs11_import.rs`       | `mod proofs11_import;`          | `workbench/project_view_actions.rs`, inside `mod tests`, next to `    mod proofs7_tree_options;`                                                                                                                      |
| `debugger/state/tests/proofs11_execution_state.rs`              | `mod proofs11_execution_state;` | `debugger/state.rs`, inside `mod tests` (line ~2045, the one starting `    use super::testing::{Reply, World, …};`), after its `use` lines. The folder `debugger/state/tests/` is new.                                |
| `platform/keymap/tests/proofs11_host_modifier.rs`               | `mod proofs11_host_modifier;`   | `platform/keymap.rs`, inside `mod tests`, next to `    mod proofs_presets;`                                                                                                                                           |

## Hunks the tests depend on

### H1 (Docker tests): reach the Services state and its Docker store

`workbench/services_window.rs`: the fields `ServicesWindow::state` and `ServicesState::docker` are private to that file and it has no `mod tests`. Replace the existing test-only impl (anchor: `#[cfg(test)]\nimpl ServicesWindow {` at line ~69) with:

```rust
#[cfg(test)]
impl ServicesWindow {
    pub(super) fn detail(&self) -> Option<&Entity<ServiceDetailPane>> {
        self.detail.as_ref()
    }

    /// What the window's tree is drawn from.
    pub(super) fn state(&self) -> &Entity<ServicesState> {
        &self.state
    }
}

#[cfg(test)]
impl ServicesState {
    /// The Docker store, for a test that answers its reads itself instead of the window.
    pub(super) fn docker_mut(&mut self) -> &mut DockerStore {
        &mut self.docker
    }
}
```

All three tests of `proofs11_docker.rs` depend on H1. (Checked: expanding a Docker row in the view asks the window for nothing, `ServiceRequest::Expand` is ignored in `services_view_event`, so `docker_rows`'s `expand_all` starts no read.)

### H2 (hot swap tests): answer the hot swap compile in a test

`debugger/driver.rs`. After the existing `#[cfg(test)] impl gpui_kit::Global for JavaDebugServerPort {}` (line ~82) add:

```rust
/// What every hot swap compile answers in a test, and after how long, in place of jdtls'
/// `java/buildWorkspace` (which needs a JDK and a Java project).
#[cfg(test)]
#[derive(Clone, Copy)]
pub struct JavaBuildAnswer {
    pub status: ide_lsp::jdtls::JavaBuildStatus,
    pub after: std::time::Duration,
}

#[cfg(test)]
impl gpui_kit::Global for JavaBuildAnswer {}
```

and in `fn call`, replace the arm `BackendCall::JavaBuildWorkspace { module, request } => self.ask(…),` (line ~803) with:

```rust
            BackendCall::JavaBuildWorkspace { module, request } => {
                #[cfg(test)]
                if let Some(answer) = cx.try_global::<JavaBuildAnswer>().copied() {
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(answer.after).await;
                        let _ = this.update(cx, |driver, cx| {
                            driver.with_host(cx, |debugger, host| {
                                debugger.hot_swap_built(ticket, Ok(answer.status), host);
                            });
                            driver.pump(cx);
                        });
                    })
                    .detach();
                    return;
                }
                self.ask(
                    cx,
                    move |host, label| async move {
                        java::java_build_workspace(&host, &label, PathBuf::from(module), Some(request)).await
                    },
                    Self::on_debugger(move |debugger, result, host| debugger.hot_swap_built(ticket, result, host)),
                );
            }
```

(The delayed answer is what lets Cancel Hot Swap Build run while the compile is under way: `HotSwap::cancel_build` only marks it cancelled, and the answer that comes afterwards ends the reload as cancelled, as with jdtls. The `CancelRequest` the cancel sends reaches `host.cancel_request` for an id nothing holds, which is harmless.) Both tests of `proofs11_hotswap.rs` depend on H2.

### H3 (host-modifier tests): the primary modifier of a host

`platform/keymap.rs`, next to `host_binding` / `mac_stroke` (line ~75):

```rust
/// Whether `modifiers` hold the primary modifier: ⌘ on macOS, Ctrl elsewhere (the Windows key is not it).
pub fn has_primary_modifier(modifiers: &gpui_kit::Modifiers, mac: bool) -> bool {
    if mac { modifiers.platform } else { modifiers.control }
}
```

This is the webview's `hasPrimaryModifier`. Today the same test lives as private copies (`run/view.rs::primary`, `debugger/variables_view.rs::primary`, both on `cfg!(target_os)`); switching them to it is optional and not needed by the tests. Both tests of `proofs11_host_modifier.rs` depend on H3; they repeat the existing `proofs_presets::{macos_keeps_cmd, windows_uses_ctrl_and_spells_shortcuts_out}` with the missing asserts, so the integrator can delete those two old ones once these pass.

## Tests, rows they prove

### `workbench/tests/proofs11_vcs_windows.rs` (3 tests)

- `add_to_gitignore_reloads_the_project_tree_with_the_file_ignored` → `unit:src/features/vcs/__tests__/ignore.test.ts` (spec case "routes all paths to gitignore then refreshes status and tree", the tree half): after `vcs.ignoreShared` the Project view row of the file has `node.ignored` and the status no longer lists it.
- `add_to_local_exclude_refreshes_the_status_and_the_project_tree` → same row (case "routes all paths to localExclude then refreshes status and tree"): after `vcs.ignoreLocal` the exclude file has the rule, the status drops the file and the tree row turns ignored.
- `refreshes_after_a_failed_or_partially_completed_multi_repository_write` → same row (spec case of that name): a `!/kept.log` rule in `.gitignore` makes `ide_vcs::add_to_ignore` save both rules in `.git/info/exclude` and then fail its verification; the test asserts the "Add to .git/info/exclude failed" notice, the rules on disk, `other.log` gone from the status and drawn ignored in the tree, `kept.log` still listed, no editor opened.
- With all three passing the row becomes **done** (the 7 model cases plus the reload/refresh are then asserted through the window).

### `workbench/shelf/tests/proofs11_shelf.rs` (2 tests)

- `copies_only_the_selected_file_patch_through_the_scoped_patch_command` → `unit:src/features/vcs/__tests__/shelf.test.tsx` (cases "copies only the selected file patch…" and the context-menu half of "preserves file targets through selection, menus, toolbar, and activation"): a right click on `a.txt` of a shelf holding `a.txt` and `b.txt` opens the shelf menu (rows asserted, Rename… disabled); clicking Copy as Patch to Clipboard puts a patch with `a.txt` and without `b.txt` on the clipboard.
- `routes_the_configured_show_diff_shortcut_to_the_focused_shelf_selection` → same row (case of that name): the IntelliJ presets bind `vcs.showDiff` to `Meta+D` / `Ctrl+D`; the key pressed in the Shelf tab (`shelfFocus` on) opens the diff window with the shelved base; pressed in a focused editor (`editorTextFocus` on) no diff opens and the line is duplicated (`code.duplicateLine` shares the key, `vcs.showDiff` has `keyWhen: !editorTextFocus` in `assets/gen/actions.json`).
- The row stays **partial**: not asserted through the window are skipping a missing or binary local file in Compare with Local and the Unshelve change-list path mapping across roots (still only on the model).

### `workbench/vcs_operations/tests/proofs11_worktrees.rs` (3 tests)

- `disables_removal_of_the_worktree_containing_project` → `unit:src/features/vcs/__tests__/worktreesPanel.test.tsx`: the project opened in a linked worktree; in the Worktrees dialog its row is `current…`, choosing it leaves `remove` disabled with the tooltip "Cannot remove a worktree containing the open project.", pressing it asks nothing and the folder stays.
- `keeps_the_main_worktree_protected` → same row: the main worktree's row reads `main…`, Remove disabled, Open enabled, nothing removed.
- `marks_both_opened_roots_current_while_grouping_their_repository_once` → same row: a project folder holding `app/` (repository) and `app-feature/` (worktree linked to it) has two VCS roots; the dialog lists 3 rows (the repository once, two worktrees at depth 1), both worktrees `current…`.
- With the three the row becomes **done** (all 5 spec cases through the dialog, with `new_worktree_creates_one_that_the_worktrees_dialog_lists_and_removes`).

### `workbench/services_actions/tests/proofs11_docker.rs` (3 tests, all need H1)

No Docker program runs and no daemon is reached: the docker settings file keeps no connection (`services_project`), the test adds a connection to the store itself, takes the `Snapshot` read the store asks for out of the returned requests instead of letting the window carry it out, and answers it with a made-up `DockerSnapshot`. The events stream request that follows is dropped unanswered, so the store keeps its events ticket and the 10 s poll skips the ready connection. The connection's socket is a path in a temporary folder no daemon listens on.

- `refresh_docker_reads_the_connection_again_and_the_tree_shows_what_it_found` → `action:docker.refresh`: run through the registry while the first read is under way, the store queues a second read (the window carries out nothing); answering the first asks for exactly one more, answered with a snapshot holding a new container, which the Services tree then shows (`docker:fake:container:fresh-id`). Becomes **done**.
- `show_stopped_containers_leaves_the_stopped_container_out_of_the_tree` → `action:docker.toggleStoppedContainers`: the stopped container's row goes and comes back, the running one and the images stay. Becomes **done** (with the existing storage test).
- `show_untagged_images_leaves_the_untagged_image_out_of_the_tree` → `action:docker.toggleUntaggedImages`: the `<none>:<none>` image's row goes and comes back, the tagged one and the containers stay. Becomes **done**.

### `workbench/debug/tests/proofs11_hotswap.rs` (2 tests, need H2)

The Java session is a Remote JVM Debug configuration in attach mode started with `debug.startConfig` (as `proofs6_debug.rs` starts its listen-mode one); `JavaDebugServerPort` points the debugger at a loopback server of the test that runs the fake adapter (`FAKE_ADAPTER` with `attach` handled as a launch of `main.fake`, and `redefineClasses` answered with `changedClasses: ["com.example.App"]` after writing a file `redefined` in the project). The session's adapter is `java-debug`, which is what makes `caps.hot_code_replace` true (`ide_dap::HOT_CODE_REPLACE_ADAPTERS`).

- `reload_changed_classes_compiles_then_reloads_the_classes_into_the_java_session` → `action:debug.reloadChangedClasses` (**done**): `debugCanHotSwap` OFF before the session and ON with it; run through the registry, the compile (answered `Succeeded`) is followed by `redefineClasses` at the adapter (the file `redefined` appears) and the notice "1 class reloaded"; `debugHotSwapBuilding` OFF afterwards, the session keeps running. Also reads `context:debugCanHotSwap` ON and OFF, if that row is still partial.
- `cancel_hot_swap_build_aborts_the_reload_before_any_class_is_redefined` → `action:debug.cancelHotSwap` (**done**): the compile answers after 5 s of the test clock; while it runs `debugHotSwapBuilding` is ON and "Compiling changed classes…" shows; Cancel Hot Swap Build through the registry ends it with "Hot Swap cancelled", the key OFF, no `redefined` file, no "reloaded" notice. Reads `context:debugHotSwapBuilding` ON and OFF.

### `workbench/editor_core/tests/proofs11_read_only.rs` (2 tests)

- `a_binary_file_is_left_alone_by_the_read_only_toggle` and `a_too_large_file_is_left_alone_by_the_read_only_toggle` → `module:src/editor/core/fileAttributes.ts`: with a text file opened first and then the binary file (NUL bytes) or a 50 MiB + 1 KiB text file (a sparse tail past `ide_vfs::READ_ONLY_THRESHOLD`) in the active tab, `file.toggleReadOnly` (its `when` is `textEditorActive`, so the registry may refuse it with the hex viewer active; the test calls `execute_action_or_throw` and accepts either) asks nothing, neither file turns read-only on disk and the text editor stays writable. Natively both files open in the hex viewer (`features_b::viewers::file_kinds::wants_hex_view`), so the window has no text editor for them and `Workbench::toggle_read_only` returns at `self.editor()`; the webview leaves them alone through its `binary` / `tooLarge` banners. Same outcome, other path.
- The row stays **partial**: the virtual-document case is not written (see below). The status item's click and the failing chmod are already proven by `proofs7_read_only`.

### `workbench/plugin_contributions/tests/proofs11_plugin_debug.rs` (1 test, unix)

- `the_debug_preparation_changes_the_program_and_the_session_launches_the_changed_one` → `action:debug.pluginStart`: the fake adapter prints `launched <program>` to the debug console on `launch`; the detector's command is the bare `execution-tool`, and `PluginHost::prepare_debug_run` (`crates/ide-host/src/plugins/execution.rs`) replaces it with the program granted to the plugin. Without the preparation `debugger::launch::resolve_command` would keep `execution-tool` (no `/` in it), so the console line `launched /…/bin/execution-tool` can only come from the change. Row stays **partial**: "Debug from the configuration chooser goes through the attach" (Java) is not covered.

### `workbench/project_view_actions/tests/proofs11_import.rs` (1 test)

- `uses_project_copy_for_a_local_destination` → `unit:src/features/project-tree/fs.test.ts` (that case): a file the file manager put on the clipboard (`ClipboardEntry::ExternalPaths`) pasted with `project.paste` into `src/` of a local project is copied there (`FsRequest::ImportLocal` → `BackendCall::FsCopy`, `project_view::fs::backend_call`), the original stays, the tree lists the copy. Row stays **partial**: its other three cases need a remote window (see below).

### `debugger/state/tests/proofs11_execution_state.rs` (1 test)

- `retains_locals_whose_names_are_object_prototype_properties` → `unit:src/features/debugger/__tests__/executionState.test.ts`: all twelve names `Object.getOwnPropertyNames(Object.prototype)` gives are kept in `execution_point().locals` with their value. The case was in fact already covered by `debugger::state::tests::retains_locals_whatever_their_names` (six of the twelve names, under another name), which the sweep did not find. With this one the row becomes **done** (10 of 10).

### `platform/keymap/tests/proofs11_host_modifier.rs` (2 tests, need H3)

- `macos_keeps_cmd`, `windows_uses_ctrl_and_spells_shortcuts_out` → `unit:src/platform/keymaps/windows.test.ts`: the existing asserts plus `has_primary_modifier` (⌘ on macOS; Ctrl and not ⌘ elsewhere). Row becomes **done** (6 of 6).

## Not written, and why

- `module:src/editor/core/fileAttributes.ts`, virtual document case: no window test can open a virtual document (`jdt://…`) without jdtls. Also the native toggle never treats one as virtual: `Workbench::toggle_read_only` (`workbench/editor_core.rs:315` and `:327`) passes `virtual_document: false` and `banner: None`. Fix when it becomes reachable: `virtual_document: crate::platform::virtual_docs::is_virtual_uri(&path)` in both places.
- `unit:src/features/project-tree/fs.test.ts`, the three remote cases (upload with cancel of the client request, a disconnected upload, one task for a batch): they need a remote window (an `openidea-server` over SSH); in a local window `Effect::BeginLocalImport` / `FinishLocalImport` are ignored on purpose (`workbench/project_files.rs:668`).
- `unit:src/features/project-tree/unityRefresh.test.ts` (both cases): the native window has no Unity view. `project_view::index::UnityRefresh` and `sync_project` have no caller outside `project_view/index.rs` (grep), and `ProjectTree` (`project_tree.rs`) keeps no Unity metadata, so there is no state a window test could read. The missing behaviour would go in `Workbench::backend_event`'s `IdeEvent::FsChanged` arm: feed `UnityRefresh::fs_changed`, and on `expire` read `project_current` and apply it with `sync_project`, dropping a reply whose `UnityRead.root` is no longer the project's.
- Group 2 (diff view spec cases): **not started**; no test written. What a first pass found (grep of `<spec>.test.tsx ›` citations in `app-gpui/src`, names only, bodies not read):
  - `diffDialog.test.tsx`: cited are "keeps the changes tree mounted underneath…", "moves along the chain…" (`vcs_diff::dialog_view::tests::moves_along_the_chain_inside_the_dialog_instead_of_opening_editor_tabs`), "closes on Jump to Source…". Uncited: "opens the diff in a front dialog, not in an editor tab", "opens the context menu's Show Diff in a front dialog…", "remembers the resized size for the next opening (#2328)", "owns the close shortcut with %s focus" (body, footer, titlebar), "opens an editor tab instead once the non-modal interface is chosen". Some may exist uncited (e.g. `workbench::vcs_operations::tests::open_diff_shows_the_diff_of_its_argument`).
  - `diffToolbar.test.tsx`: uncited are "jumps to the working copy of a file shown at a revision", the two `it.each` blocks at spec lines 223 and 238 and "turn on from the settings menu and stay on" (282), `it.each([null, "bottom"])` (316), "does not offer editor maximization in an embedded diff", "restores the shared layout after the diff tab is remounted", "maps a line of the commit to the working copy", "maps a line of the parent through the diff to the working copy", "jumps on ⌘↓ in the IntelliJ macOS keymap only while a diff pane has focus", `it.each` at 504.
  - `movedCode.test.tsx`: cited only the three view cases ("explains ignored whitespace…", "omits inner edits in Highlight lines mode", "respects Do not highlight"). Its ten model cases (spec lines 49–177) are the job of `vcs_diff::comparison::moves::find_moved_blocks`, whose own tests (`finds_a_block_that_moved_unchanged`, `finds_a_block_that_moved_with_an_edit`, …) are not matched to them; "labels edited moves…" and "shows native move links, follows them in both directions…" are view cases with no citation.
- `unit:src/features/search/__tests__/findPopup.test.tsx`: matched case by case below from the native tests' doc comments (a reading, not a run; hypothesis until someone checks the asserts); the missing ones are **not written** (not reached).

### findPopup.test.tsx, case by case (25 `it`)

| #   | Spec case                                                                                         | Native test (`find::popup_view::tests::…`)                                                                                                                                                                  | Verdict                                                                                   |
| --- | ------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| 1   | ⇧↓ extends the selection; ↵ in Replace mode replaces the selected rows                            | `the_keyboard_walks_the_results_and_enter_opens_the_match` (⇧ extends), `replace_replaces_the_selected_row_and_moves_on` (one row)                                                                          | partial: Enter replacing several selected rows not asserted                               |
| 2   | ⌘-click toggles rows into the selection                                                           | `the_mouse_selects_rows_and_the_preview_follows` ("with the primary modifier besides the others")                                                                                                           | partial: a plain click back to one row not named                                          |
| 3   | ⇧⌥↵ replaces all, asking first only when several files are affected                               | `replace_all_asks_first_when_several_files_change`, `replace_all_in_one_file_does_not_ask` (button and ⌥A)                                                                                                  | partial: the ⇧⌥↵ key itself                                                               |
| 4   | ⌥ mnemonics toggle Preserve Case and the file mask                                                | `the_file_mask_limits_the_files_searched` (⌥K)                                                                                                                                                              | partial: ⌥E Preserve Case not found                                                       |
| 5   | the pin is a remembered toggle that keeps the popup open when a result is opened                  | none found (the view has `pinned` state)                                                                                                                                                                    | missing test                                                                              |
| 6   | find-next keys move the selection and ⌥↓ lists recent searches                                    | `the_history_lists_the_recent_searches_and_replacements`                                                                                                                                                    | partial: `find.popup.nextResult` / `previousResult` not named                             |
| 7   | the Directory scope's browse button picks the folder, starting at the current one                 | `the_scope_tabs_decide_where_the_search_looks` (directory starts at the edited file's folder)                                                                                                               | partial: Browse… not named                                                                |
| 8   | closing remembers the search text                                                                 | `a_closed_popup_is_remembered_by_the_next_one`                                                                                                                                                              | covered                                                                                   |
| 9   | marks retained rows immediately in replace=%s and waits for the first new file                    | `searches_as_you_type_once_the_query_has_rested` ("results are being updated")                                                                                                                              | partial                                                                                   |
| 10  | blocks Replace / Replace All / Enter / All shortcut / All mnemonic during debounce and validation | none found (`is_stale` guards exist in `replace`/`replace_all`)                                                                                                                                             | missing test                                                                              |
| 11  | invalidates when search options change (6 variants)                                               | `the_option_toggles_search_again_with_them` (case, word, regex), `the_file_mask_limits_the_files_searched`, `the_scope_tabs_decide_where_the_search_looks`, `the_search_context_says_where_a_match_may_lie` | partial: the invalidation (stale rows) itself not named                                   |
| 12  | clears retained rows and the count when the new search finds nothing                              | none found                                                                                                                                                                                                  | missing test                                                                              |
| 13  | keeps opening an outdated result available in replace=%s                                          | none found                                                                                                                                                                                                  | missing test                                                                              |
| 14  | rejects Replace All if the query changes while confirmation is open                               | none found                                                                                                                                                                                                  | missing test                                                                              |
| 15  | remembers Open results in new tab and passes it on; ⌥B toggles it                                 | `open_in_find_window_hands_the_search_over` ("kept between sessions")                                                                                                                                       | partial: ⌥B                                                                               |
| 16  | keeps OpenIDEA's Replace preview: a diff in place of the code preview                             | `the_replace_preview_shows_the_change`                                                                                                                                                                      | covered                                                                                   |
| 17  | names the result keys only for a multi-line query                                                 | none found                                                                                                                                                                                                  | missing test                                                                              |
| 18  | ⌥P / ⌥M / ⌥D / ⌥S pick the scope tab, whose letter is underlined off macOS                        | none found by name                                                                                                                                                                                          | missing test                                                                              |
| 19  | turned on with nothing typed takes the first suggestion                                           | `an_empty_file_mask_takes_the_most_recent_one`                                                                                                                                                              | covered                                                                                   |
| 20  | shows the preview only once something matches                                                     | none found                                                                                                                                                                                                  | missing test                                                                              |
| 21  | is resizable and opens at the size it was left at                                                 | none; `resiz` does not occur in `find/popup_view.rs`                                                                                                                                                        | behaviour likely missing natively (hypothesis: `grep -n -i resiz app-gpui/src/find/*.rs`) |
| 22  | asks for a query while there is none                                                              | `empty_results_say_which_options_were_used` (probably)                                                                                                                                                      | partial                                                                                   |
| 23  | names the one option in use and clears it                                                         | `empty_results_say_which_options_were_used`                                                                                                                                                                 | covered                                                                                   |
| 24  | lists every option in use with its key, and ⇧⌥⌫ clears them all                                   | `empty_results_say_which_options_were_used`, `the_option_toggles_search_again_with_them` (⇧⌥⌫)                                                                                                              | covered                                                                                   |
| 25  | offers Search with Regex for a query that reads as one                                            | `empty_results_say_which_options_were_used`                                                                                                                                                                 | covered                                                                                   |

## Expected to fail on first run

1. `add_to_local_exclude_refreshes_the_status_and_the_project_tree` and `refreshes_after_a_failed_or_partially_completed_multi_repository_write`, at "the tree to be read again". The webview's `addToIgnore` calls the project tree's `reloadAll()` after every write, failed or not. Natively `Workbench::add_to_ignore` (`workbench/vcs_operations.rs`) only gets the VCS refresh inside `run_op`; the project tree re-lists a folder only on an `IdeEvent::FsChanged` touching it (`project_tree.rs::backend_event`), and a write to `.git/info/exclude` (or a failure) brings none for the project's top folder. The `.gitignore` case is likely rescued by the `FsChanged` of `.gitignore` itself. Fix (in `add_to_ignore`, after `run_op` returns, on both `Ok` and `Err`, before `finished`):
   ```rust
   let _ = this.update(cx, |workbench, cx| {
       if let Some(tree) = workbench.project_tree().cloned() {
           tree.update(cx, |tree, cx| tree.reload_all(cx));
       }
   });
   ```
   (`ProjectTree::reload_all` exists: "Re-reads every loaded folder"; adjust to the window's own accessor if `project_tree()` is not the right one there.)
2. `marks_both_opened_roots_current_while_grouping_their_repository_once`: depends on `ide_vcs::Repos::discover` listing the linked worktree `app-feature/` (it has a `.git` file, which `path.join(".git").exists()` accepts) and on the window reading all roots (`state.roots.len() == 2`); and on the form marking a root current when it is among the roots while the project root (the plain parent folder) is not a worktree. Hypothesis, not checked.
3. The Docker tests if `sync_services_window` has not run the store's `watch()` by the time the settings are read: `add_connection` then gets no read back and its `assert_eq!(reads.len(), 1, …)` fails with that message. The settings load only starts from `watch()`, so the wait on `settings().is_some()` should cover it.
4. `routes_the_configured_show_diff_shortcut_to_the_focused_shelf_selection`, editor half: assumes the editor has `editorTextFocus` once focused and that `code.duplicateLine` wins the shared key there.
5. The hot swap tests, at "the running Java session": depend on `JavaFlow::attach` getting through its steps (Kotlin scan, project name) in a temp project without jdtls before it asks for the debug server, and on ide-dap connecting to the `debugServer` port for an attach request. `plugin_java_debug.rs` proves the same server path for a launch from a plugin, not for a Remote JVM Debug attach. If it stops earlier, the notice "Cannot start debugging" says why.
6. `uses_project_copy_for_a_local_destination`: whether the paste of one external file opens the Copy dialog first (the test presses Enter if it does), and whether `os_clipboard_read` accepts files the test clipboard holds.
7. `the_debug_preparation_changes_the_program_and_the_session_launches_the_changed_one`: an `output` event sent before `initialized` must reach the session's console (the debugger may drop output of a session not yet configured; then move the event after `event('initialized')`).
8. `a_too_large_file_is_left_alone_by_the_read_only_toggle` creates a 50 MiB sparse file in the temp folder (on APFS a hole costs no disk); if the hex viewer reads the whole file the test is slow, not wrong.

## Tests needing no hunk vs. needing one

No hunk: `proofs11_vcs_windows` (3), `proofs11_shelf` (2), `proofs11_worktrees` (3), `proofs11_read_only` (2), `proofs11_plugin_debug` (1), `proofs11_import` (1), `proofs11_execution_state` (1). Need H1: `proofs11_docker` (3). Need H2: `proofs11_hotswap` (2). Need H3: `proofs11_host_modifier` (2). Total 20 tests.
