I finished Part 0 and both stages, and clippy is clean, but one full-suite run out of 13 failed and I have not found why. The failing test is `workbench::tests::the_merge_editor_opened_from_the_banner_runs_its_actions_and_the_rebase_is_continued_from_the_notice`. By your rule that is not done.

## 1. Suite, clippy, compile errors, scope, Part 0 outputs

- **Suite ×5:** four runs gave 6456 passed, 0 failed, 0 ignored, about 52 s each. Run 3 gave 6455 passed and 1 failed, the merge test above. It panicked in `wait_long` at `workbench/mod.rs:7428` (a 30-second wait ran out), and that run took 87 s.
- **Reruns:** 8 more full runs (3 through cargo, 5 by running the test binary directly), 6 runs of `workbench::`, and 8 runs of that test alone were all green.
- **Three test binaries running at the same time:** the merge test passed in all three. Four `workbench::vcs_operations` tests failed in each, all on file-watcher waits ("the change is read", "the file is read"). Those waits happen before any editor opens, so they are not on code I changed.
- **Cause of the merge failure: hypothesis.** Most likely one of its watcher-driven 30-second waits (the conflicts being resolved, the rebase going on) stalling under machine load; the load average reached 26 during these runs. To confirm, run `target/debug/deps/openidea_gpui-7e4f14697984a448 the_merge_editor 2>&1 | grep "did not happen"` in a loop while the machine is loaded.
- **Clippy** (`--all-targets -D warnings`): clean after stage 2.
- **Compile errors:**
  - Part 0: 0.
  - Stage 1: 1, an `E0599` in `editor_core.rs` (`cx.new` without the `AppContext` trait imported; fixed by importing `AppContext as _`).
  - Stage 2: 0.
- **Scope:** I ran rustfmt only on files I edited; `git status` showed nothing else of mine changed. Other sessions committed during my run (HEAD is now `70807d43`, including the `proofs_merge_*` drafts); I did not touch those.
- **Part 0 commands:**
  - `cargo test -p ide-ssh -p ide-db -p ide-host`: exit 0, 62 test binaries, 787 passed, 0 failed, 17 ignored (all were `#[ignore]` before, including `os_credential_store`).
  - `cargo test -p openidea export_bindings`: 1 passed.
  - `git status --short src/ src-tauri/`: printed nothing.

## 2. Part 0

**Diff by file:**

- **`crates/ide-ssh/src/secrets.rs`** (+200 −19):
  - New `keep_secrets_in_files()`, `secrets_in_files()` and `default_store_installed()`, behind one process-wide flag.
  - The keychain store now holds a reference to that flag. While it is set, it reports itself unavailable and `get`/`set`/`delete` return `NoDefaultStore`.
  - New file store: the secrets go in `<index>.vault`, mode `0600`, next to the index. A corrupt vault makes the save fail and is left untouched.
  - `SecretFile::with_legacy` picks the file store once the switch is on, and the index and the vault share one owner-only write helper.
- **`crates/ide-ssh/tests/secrets_in_files.rs`** (new, 31 lines): a separate test binary, because the switch can never be turned off.
- **`app-gpui/src/backend.rs`:** `AppDirs.isolated` is true from `under()` and false from the platform directories. `Backend::start` turns the switch on before `git_creds::init`.
- **`app-gpui/src/plugins.rs`:** `KeychainSecrets` is installed only when the home is not isolated.

No new dependency, no `specta` type touched, the Tauri app never calls the switch, and the generated bindings are unchanged.

**Tests:**

- In `ide-ssh`:
  - `a_file_vault_keeps_secrets_beside_the_index_owner_only_and_reads_them_back`
  - `a_file_vault_forgets_a_deleted_secret_and_keeps_the_others`
  - `a_corrupt_file_vault_fails_the_save_instead_of_overwriting_it`
  - `the_os_store_refuses_to_work_once_secrets_are_kept_in_files`
  - `once_secrets_are_kept_in_files_saved_passwords_go_to_a_file_and_the_os_store_is_never_set_up` (the integration test)
- In the app, `backend::tests::saved_passwords_stay_in_files_under_a_temporary_home` saves through the same calls the UI makes:
  - the SSH password through `host.ssh.set_secret`, as `store_ssh_secret` does;
  - the database password through `ide_host::ops::db::set_password`, as `run_op` does for `DbOp::SetPassword`.

  It reads both back from the files, checks the vault is `0600` and the index holds no secret, and asserts `default_store_installed()` is false. `directories_follow_the_home_override` now also asserts `isolated`.

- **Hypothesis:** skipping `KeychainSecrets` for an isolated home is compiled out in test builds, so no test runs it. To confirm: start `openidea-gpui` with `OPENIDEA_GPUI_HOME` set to a temp folder, save a plugin secret, and check it appears in a file under `<home>/data`.

**Agents CLI question (by reading, not measured):**

- Listing the agents only checks whether the program is on `PATH`; it starts nothing. The app never calls `ops::agent::probe`.
- A task starts only from the New Task page's Start (click or Enter), and Start needs the chosen agent installed.
- Every test with a real backend that starts a task does so through `start_agent_task`, and all 7 of those tests call `agents_project` first, which selects the fake agent (checked by script). The only other test touching New Task has no project and asserts it is refused.
- `Draft::default()` picks `"claude"`, so a future test that starts a task without the fake agent would launch the real CLI on this machine.

## 3. Stages

The IPC column follows the notes' grep of the adapters; I did not re-check it.

**Stage 1 (paths under `editor::core_actions::tests::` and `workbench::editor_core::tests::`):**

| Actions                                                               | Test                                                                                                                                             | IPC                                     | Status                                              |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------- | --------------------------------------------------- |
| fold, unfold, toggle, all, recursive, selection, block                | `the_folding_commands_hide_and_show_the_lines_of_their_regions` (+`the_caret_skips_the_lines_a_fold_hides`, `a_fold_follows_the_edits_above_it`) | `doc_folding` → `ops::doc::doc_folding` | partial (no gutter chevron clicks, folds not saved) |
| extend / shrink selection                                             | `extend_and_shrink_selection_walk_the_syntax_ranges_around_the_caret`                                                                            | `doc_selection_ranges`                  | partial                                             |
| `code.autoIndentLines`                                                | `auto_indent_lines_reindents_the_selected_lines`                                                                                                 | `doc_edit`                              | done                                                |
| `editor.scrollToCenter`                                               | `scroll_to_center_puts_the_caret_line_in_the_middle_of_the_view`                                                                                 | none                                    | done                                                |
| font size ± / reset                                                   | `the_font_size_steps_in_the_active_editor_and_resets_in_every_editor`                                                                            | none                                    | partial                                             |
| back / forward                                                        | `back_and_forward_move_between_the_places_of_two_files`                                                                                          | none                                    | partial (only file switches)                        |
| last / next edit location                                             | `last_and_next_edit_location_walk_through_the_edits`                                                                                             | none                                    | done                                                |
| `editor.showHint`, `editor.copyPath`, `editor.copyPathWithLineNumber` | `the_copy_path_actions_copy_the_forms_of_the_edited_file`                                                                                        | clipboard                               | the hint is partial                                 |
| reload from disk / all                                                | `reload_from_disk_reads_the_file_again_unless_it_has_unsaved_changes`                                                                            | `doc_set_encoding` (reload)             | partial                                             |
| `file.toggleReadOnly`                                                 | `toggle_read_only_asks_then_locks_the_file_and_its_editor`                                                                                       | `fs_set_writable`                       | done                                                |
| paste from history (+ dialog paste / remove)                          | `paste_from_history_pastes_the_entry_chosen_in_the_dialog`                                                                                       | `doc_edit`                              | partial                                             |

Stage 1 shortcut tests:

- `the_editor_actions_do_nothing_outside_the_editor`: with the Project view focused, ⌘- ⌥↑ ⌥↓ ⌘. ⌘⇧. ⌘⇧- ⌘⌥- ⌃⌥I ⌃L ⇧⌘V, and with a fold present ⌘= ⌘⇧= ⌘⌥=, change nothing; ⌘- and ⌘= work in the editor.
- `the_actions_need_what_their_when_clauses_name`: in the single-file window ⌥⇧. ⌥⇧, ⇧⌘⌫ ⌥⌘Y do nothing.
- Positive presses of ⇧⌘⌫ and ⌥⌘Y were added to the last-edit and reload tests.

**Fold test:** `workbench::plugin_contributions::tests::a_fold_above_moves_every_line_below_it_to_its_row`. It uses the official test-runner plugin's gutter icons on lines 5 and 6, and Fold Selection hides lines 2–4. With the fold in place it proves:

- each line from 5 to 11 is positioned at row line − 3, and a hidden line has no position;
- a click on line 6 puts the caret on line 6;
- ↓ from the fold's header goes to line 5 and ↑ comes back;
- Page Down lands three lines further than it does without the fold, and Page Up returns to line 0;
- the breakpoint tint and dot (line 7), the execution-line tint (line 8), the error underline (line 9) and both plugin icons are painted inside their own row;
- typing at the end of line 0 keeps the fold;
- a wheel scroll to the end paints the last line flush with the bottom of the view, and ⌘↓ moves the caret to the last line and scrolls it into view.

**Stage 2 (paths under `workbench::editor_language::tests::`):**

| Actions                                  | Test                                                                                                               | IPC                                                                    | Status                         |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------- | ------------------------------ |
| `refactor.rename`                        | `rename_applies_the_servers_edit_to_both_files_and_one_undo_restores_each`                                         | `lsp_prepare_rename`, `lsp_rename`, apply / undo / redo workspace edit | partial                        |
| reformat, reformat file                  | `reformat_code_applies_the_servers_edits_and_reports_the_formatted_lines`                                          | `lsp_format`                                                           | partial                        |
| reformat with Prettier / Oxfmt / Ruff    | `reformat_with_a_named_formatter_runs_that_formatter`                                                              | `lsp_format`                                                           | done                           |
| optimize imports (+file), cleanup file   | `optimize_imports_and_code_cleanup_apply_the_servers_source_actions`                                               | `lsp_code_actions`, `lsp_apply_code_action`                            | optimize done, cleanup partial |
| generate / implement / override          | `generate_lists_the_generate_actions_and_applies_the_chosen_one` (+`cmd_n_in_a_markdown_file_opens_the_new_popup`) | same two                                                               | partial                        |
| `code.applyQuickFix`                     | `apply_quick_fix_applies_the_fix_of_the_problem_at_the_caret`                                                      | same two                                                               | partial                        |
| next / previous error, error description | `next_error_moves_the_caret_to_the_next_problem_and_shows_its_message`                                             | none                                                                   | done                           |
| highlight usages, next / previous usage  | `highlight_usages_marks_the_occurrences_and_the_caret_moves_through_them`                                          | `lsp_document_highlights`                                              | partial                        |
| quick definition / type definition       | `quick_definition_shows_the_definitions_lines_and_f4_opens_it` (+3 `editor_lsp::definition_view::tests`)           | `lsp_goto`                                                             | partial                        |
| `navigate.gotoTest`                      | `opens_the_only_test_of_a_class`                                                                                   | `project_roots`, `find_files`                                          | partial                        |
| next / previous method                   | `moves_by_line`                                                                                                    | `doc_outline`                                                          | done                           |

Stage 2 shortcut test: `the_language_keys_do_nothing_while_a_tool_window_has_the_keyboard` now also presses ⇧F2 ⌃⇧↑ ⌃⌥↑ ⌃I ⌃O ⌥Space ⌘Y ⇧⌘T ⇧F6 ⌘N.

**Hypothesis:** for ⌃I, ⌃O, ⌥Space, ⌘Y, ⇧⌘T, ⇧F6, ⇧F2, ⌃⇧↑ and ⌃⌥↑ no test presses the key in the editor, so these negative checks are unproven. Pressing each in the editor in its own action's test would confirm them.

## 4. Fixes, changed tests, notes that were false

**Product changes beyond the notes:**

- `Editor::painted_marks(kind)` and `Editor::painted_bounds()`, with mark positions recorded during paint, so the fold test can assert where marks land.
- The `AppContext` import in `editor_core.rs`.
- Two closures in `editor_language.rs` split so the lines fit in 120 columns.

**Literal row lists:**

| List                 | Before                                                                                                         | After                                                         |
| -------------------- | -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| `ROOT`               | `[Show Context Actions, Paste, Copy / Paste Special, Column Selection Mode, Go To, Send to Agent, Git]`        | `[…, Go To, Folding, Rename…, Generate…, Send to Agent, Git]` |
| `GO_TO`              | `[Declaration or Usages, Implementation(s), Type Declaration, -, Find Usages, Call Hierarchy, Type Hierarchy]` | `"Test"` added after `"Type Declaration"`                     |
| Plugin entries test  | `…Go To, Send to Agent, Hello: Word Count of Active File, Hello, Git`                                          | `Folding, Rename…, Generate…` added after `Go To`             |
| Copy / Paste Special | `[Copy Path/Reference…, Copy Reference, Paste as Plain Text]`                                                  | `"Paste from History…"` added                                 |

All of these follow the editor menu in `src/editor/core/controller.ts` (Copy / Paste Special, the Go To submenu, the Folding submenu, then Rename…, Refactor, Generate…).

`GIT`, `the_tab_menu_acts_on_the_tab_it_was_opened_on`, `a_tabs_menu_closes_pins_and_splits` and the `missing` list are unchanged and pass. The tab menu (`workbench/mod.rs` `tab_menu`) uses none of the newly registered ids.

Other test changes: the last-edit test now presses ⇧⌘⌫ instead of its first registry run, the reload test adds a ⌥⌘Y press, and the negative tests are extended as listed in section 3.

**False in the notes:** "no test asserts a fixed list of those rows" — the `ROOT` and `GO_TO` constants in `context_menus.rs` do. Separately, ⌘End is not handled by the editor; ⌘↓ is the document-end key.

## 5. Left out and not built

- No drafted test was left out.
- Not built: soft wrap, unwrap, in-place rename, Refactor This…, and the optional ⌘Z hunk (2.7, in `view.rs`). Undo through the `edit.undo` action does take a refactoring back.
- Unmeasured: the cause of the merge-test flake (section 1) and the plugin keychain skip (section 2).
