Every item landed and converged: slice A first, then M3, M4, M5, M1, M2 and M8. On the final tree both whole-suite runs pass, the 32 added, declared and changed tests passed 20 of 20 during the second run, and web_view, clippy and `cargo build` are clean. Nothing was committed, no git command changed anything, and nothing under `crates/`, `docs/`, `src/`, `src-tauri/` or `e2e/` was touched. No process of mine is left (checked with `pgrep -fl "openidea_gpui-|cargo "`, which printed nothing).

## 1. Gates (final tree; logs in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dt1ai/`)

- **Suite run 1**, `.../dt1ai/suite3.log`: `test result: ok. 8231 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 213.47s`. `uptime` before: load 2.83 7.72 10.42; after: 119.01.
- **Suite run 2**, `.../dt1ai/suite4.log`: `test result: ok. 8231 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 230.56s`. Load before: 100.86 (it started right after run 1); after: 10.73.
- **20 repeats** during run 2, `.../dt1ai/repeat.log`, run by `.../dt1ai/repeat.sh` (the test binary called directly with the filters):
  - 20 of 20 runs printed `test result: ok. 32 passed; 0 failed; ...; 8199 filtered out`; no test failed even once.
  - The 32 tests are every `proofs39` test plus the 5 existing tests whose expectations changed.
  - Overlap proof: when the repeats ended, `suite4.log` had no `exit` line yet.
- **web_view**, `.../dt1ai/web_view2.log`: `test result: ok. 1 passed; 0 failed; ...; finished in 4.42s`.
- **clippy** `--all-targets -- -D warnings`, `.../dt1ai/clippy2.log`: exit 0.
- **`cargo build`**, `.../dt1ai/build2.log`: exit 0, 0 warnings.
- **An earlier full pass** (before the last test was added) also passed: `suite1.log` 8230 passed, `suite2.log` 8230 passed, 20 of 20 repeats, web_view and build clean.
- **Failures seen along the way:**
  - The first clippy run failed on `single_range_in_vec_init` at `proofs39_completion_enter.rs:23`. That log was overwritten. I fixed it with a per-test `#[expect(..., reason = "a caret is a selection of one range")]`, as `templates/editor.rs` does for its tests.
  - Both suite calls were started in the foreground and moved to the background by the harness at 180 s. No test related to the interrupt signal failed.

## 2. Slice A (an agent's edit is recorded for Edit › Undo)

**Code.** In `editor_language.rs`, an agent's edit is now recorded like a refactoring's; only the notification is left out (A1, A2, and the A3 module comment). `workspaceUndo.ts:44-46` and `workspaceHistory.ts:34-49` require this. A5 in `workbench/mod.rs` `open_project` starts the listener when the project opens; the webview does this in `src/editor/core/index.ts:42-44` and `document.ts:761-763`. I also fixed the stale comment on the `listener` field and added a `#[cfg(test)] remembered_edits()` accessor. `Content::Project` is built only in `open_project`, so no other path to an open project skips the listener.

**Tests**, all in `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/tests/proofs39_agent_undo.rs` (module `workbench::tests::proofs39_agent_undo`; 8 tests, all through the real backend, no agent started):

| Test                                                                                  | Proves                                                                                                                                                                                   | Failed before the code                                                                                                        |
| ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `does_not_announce_an_agents_edit_as_a_refactoring`                                   | Undo takes the edit back and no notification is raised                                                                                                                                   | yes                                                                                                                           |
| `edit_undo_takes_back_an_agents_edit_over_two_files_and_redo_applies_it_again`        | both files, on disk and in both editors                                                                                                                                                  | yes                                                                                                                           |
| `reports_a_refused_undo_instead_of_discarding_newer_work`                             | a closed file written by another program: the refusal names it, then Undo goes through once it is written back                                                                           | yes                                                                                                                           |
| `an_agents_edit_made_before_any_editor_opened_is_undone_from_the_editor_opened_later` | A5                                                                                                                                                                                       | yes; still failed after A1 and A2 at line 170, passed after A5                                                                |
| `each_agents_edit_is_remembered_once_before_and_after_an_editor_opens`                | history length 1, then exactly 2, so nothing is listened to twice                                                                                                                        | yes; still failed after A1 and A2 at line 263, passed after A5                                                                |
| `undoing_an_agents_edit_touches_only_the_files_it_changed_created_or_deleted`         | whole project tree compared after apply, Undo and Redo, for an edit that changes a, deletes b and creates c; a folder outside the project unchanged                                      | its first form (create only) failed before the code; the delete case was added afterwards and was not re-run without the code |
| `typing_made_before_an_agents_edit_is_kept_but_no_longer_undoable`                    | today's behaviour of earlier typing                                                                                                                                                      | yes (because the edit was not recorded at all)                                                                                |
| `an_undo_from_another_editor_keeps_newer_unsaved_typing_in_an_editor_of_the_edit`     | Undo from a while b has unsaved typing: the backend refuses with a body naming `b.txt` and ending "changed; request the refactoring again"; both editors and both files keep their bytes | added last, not run before the code                                                                                           |

The fail-first runs are `.../dt1ai/a_before.log` (7 of 7 failed) and `.../dt1ai/a_a12.log` (A1 and A2 only: 5 passed, 2 failed).

**Still different from the webview:**

- **Earlier typing.** A workspace edit clears an open editor's own history (`editor/view.rs` `remote_patch`). The text is kept, but the earlier undo steps are lost; the webview keeps them. Measured by the typing test above.
- **Refusal texts.** Only one is measured:
  - Measured: the backend's own refusal, "Cannot undo refactoring" with the body "<path> changed; request the refactoring again". The native app shows the same title and body text as `workspaceHistory.ts:107-111`. It covers a closed file changed on disk and the newer-unsaved-typing case.
  - In that second case the webview instead says "Undo newer editor changes before undoing this refactoring" (`workspaceHistory.ts:156`); natively the backend's message shows.
  - Hypothesis, read and not run: the webview's "A refactoring history operation is in progress" (`:145`), "The project for this refactoring is no longer open" (`:150`) and "The refactoring can no longer be undone" (`:152`) have no native counterpart on the Edit › Undo path. Natively a missing record makes Undo do nothing, and there is no busy flag. A test per case in `proofs39_agent_undo.rs` would settle it.
- **Wiring `editor_model/workspace_history.rs` is not contained, so it is the next slice.** It is 479 lines and used by nothing. What I read, not measured:
  - `editor/history.rs` (235 lines) has no marker step and nothing to rebase earlier entries through a change the editor did not make.
  - `DocPatched` carries only `workspace_edit: bool`, not the edit's id.
  - The fix touches `History` (a marker entry and a rebase), `view.rs` (`remote_patch`, undo and redo, about 6 places), `editor_language.rs` (`applied` and `refactoring_history` become marker-driven) and attach/detach per editor.
  - `proofs39_agent_undo_pending.rs` stays undeclared, as the brief says.
- **Outside the project.** The outside-folder assertion holds by construction for an edit inside the project. Hypothesis: the backend layer (`workspace_edit.rs:394`) does not refuse an edit that names an outside path, and the MCP layer may. A test applying such an edit through `apply_agent_workspace_edit` would settle it.

## 3. Slice B

The lists, compared by script (`.../dt1ai/compare_zig.py`); count and order are identical on both sides:

| List               | Webview | Native | Placement                                                   |
| ------------------ | ------- | ------ | ----------------------------------------------------------- |
| Zig postfix        | 12      | 12     | after Go and before Java, in the template list and the tabs |
| Zig live templates | 19      | 19     | after Go                                                    |
| Zig surrounders    | 10      | 10     | in the per-language table                                   |

The reader who said 20 live templates was wrong.

- **M3** (`postfix/model.ts:219-258, 366, 376`): `postfix_model.rs` and `postfix_model/zig.rs`. Tests: `templates::proofs39_zig_postfix::{zig_optional_unwrap_and_expansion, the_zig_postfix_set_holds_its_list_in_order}` and `settings_view::pages::editor_postfix_completion::tests::proofs39_zig_tab::the_zig_tab_lists_the_zig_templates`.
- **M4** (`live/builtins.ts:189-238, 550`): `live_builtins.rs` and `live_builtins/zig.rs`. Tests: `templates::proofs39_zig_live::{the_zig_group_holds_its_list_in_order, zig_templates_expand_with_their_defaults}` and `settings_view::pages::editor_live_templates::tests::proofs39_zig_group::{the_zig_group_follows_the_go_group, the_page_lists_the_zig_templates}`.
- **M5** (`live/surrounders.ts:146-156, 189`): `live_surrounders.rs` and `live_surrounders/zig.rs`. Tests: `templates::proofs39_zig_surrounders::{wraps_zig_in_its_parenthesised_brace_syntax, the_zig_surrounders_are_numbered_in_order}`.
- **M1** (`editor.ts:199-211`): `expand.rs` gets a `from_template` flag. A completion item's snippet leaves Enter to the editor; Tab still walks the fields in both kinds. A live template picked from the lookup goes through `TemplateEdit::new`, so it is still a template's session and Enter still walks its fields. Tests: `templates::proofs39_completion_enter::leaves_enter_to_the_editor_in_a_completions_snippet` and `editor::intelligence::tests::proofs39_completion_snippet::leaves_enter_to_the_editor_in_a_completions_snippet`. Every existing template and session test passes unchanged (256 in `.../dt1ai/m1.log`). Not run before the code.
- **M2** (`postfix/model.ts:268-276`, `editor.ts:497-502, 526-546`): `postfix_model.rs` (`action`, the description "T name = expr;"), `templates/editor.rs`, `templates/postfix_action.rs` and `workbench/editor_templates/postfix_action.rs`. Tests:
  - `workbench::tests::proofs39_java_var::{runs_introduce_variable_on_the_expression_ended_as_a_statement, falls_back_to_the_var_template_without_introduce_variable, a_declined_introduce_variable_leaves_the_var_declaration}`. I added a check to each that the backend's language-server list for the window (`lsp_servers`) is empty.
  - `templates::proofs39_java_var::{java_var_stands_for_introduce_variable, the_var_key_becomes_a_semicolon_and_the_expression_is_selected, the_var_template_replaces_the_expression_when_the_action_does_not_run}`.
  - `settings_view::pages::editor_postfix_completion::tests::proofs39_java_var_row::the_java_tab_describes_var_as_introduce_variable_does`.
- **M8** (`emmet/model.ts:31, 71-72`), model only: `emmet_model.rs`. Test: `templates::proofs39_django_emmet::expands_markup_in_django_templates_not_inside_template_tags_variables_or_comments`.

Existing expectations that changed, each required by the webview:

- `templates::postfix_model::tests::every_language_has_the_core_keys` gains zig (`postfix.test.ts:112-119`).
- `templates::postfix_model::tests::widening_stops_at_what_is_not_an_expression`: 99 → 111 templates (`postfix/model.ts:219-258`).
- `templates::live_builtins::tests::ships_the_requested_built_ins` gains the zig line (`live.test.ts:152`).
- `settings_view::pages::editor_postfix_completion::tests::{toggles_postfix_templates, options_are_stored_at_once_and_off_disables_the_rest}`: the tab line gains Zig (`postfix/model.ts:370-378`).

## 4. Rows

I confirmed every native test below as `... ok` in the suite logs. Except where marked as taken from the notes, I read each spec case against its test.

- **`unit:src/features/code-insight/workspaceUndo.test.ts` → done, 4 of 4**:
  - waits for pending typing before undoing the entire backend transaction → `code_insight::workspace_undo::tests::waits_for_pending_typing_before_undoing_the_entire_backend_transaction`
  - reports a refused undo instead of discarding newer work → `code_insight::workspace_undo::tests::reports_a_refused_undo_instead_of_discarding_newer_work`, plus the end-to-end test of the same name
  - does not apply an old notification's id to a different project → `code_insight::workspace_undo::tests::does_not_apply_an_old_notifications_id_to_a_different_project`
  - does not announce an agent's edit as a refactoring → `workbench::tests::proofs39_agent_undo::does_not_announce_an_agents_edit_as_a_refactoring`
- **`event:WorkspaceEditApplied` → stays partial.** The proof is the proofs39_agent_undo tests above. Missing: the webview's "Undo newer editor changes…" message, and earlier typing staying undoable; both need the workspace-history wiring.
- **`unit:src/features/templates/__tests__/editor.test.ts` → done, 17 of 17.** The 14 in `templates::editor::tests::*` are named after their cases (equivalence taken from the notes). The other three:
  - runs Introduce Variable on the expression, ended as a statement → `workbench::tests::proofs39_java_var::runs_introduce_variable_on_the_expression_ended_as_a_statement`
  - falls back to the var template without Introduce Variable → `workbench::tests::proofs39_java_var::falls_back_to_the_var_template_without_introduce_variable`
  - leaves Enter to the editor in a completion's snippet → `editor::intelligence::tests::proofs39_completion_snippet::leaves_enter_to_the_editor_in_a_completions_snippet`
- **`unit:src/features/templates/__tests__/postfix.test.ts` → done, 10 of 10.** The 8 in `templates::postfix_model::tests::*`, plus `templates::proofs39_zig_postfix::zig_optional_unwrap_and_expansion` and `templates::postfix_model::tests::every_language_has_the_core_keys`. The last two I read against the spec: same inputs and expectations.
- **`unit:src/features/templates/__tests__/live.test.ts` → done, 18 of 18.** These are the 18 names of the existing proof text, all present. `templates::live_builtins::tests::ships_the_requested_built_ins` now checks zig as `:152` does. Equivalence of the other 17 is taken from the notes.
- **`unit:src/features/templates/__tests__/surrounders.test.ts` → done, 4 of 4**: the 3 in `templates::live_surrounders::tests::*` plus `templates::proofs39_zig_surrounders::wraps_zig_in_its_parenthesised_brace_syntax` (read against the spec: same).
- **`unit:src/features/templates/__tests__/emmet.test.ts` → stays partial, 5 of 8.** Covered: the 4 in `templates::emmet_model::tests::*` plus `templates::proofs39_django_emmet::expands_markup_in_django_templates_not_inside_template_tags_variables_or_comments` (read: same). Not covered (need the Emmet engine's tables): expands abbreviations into snippets with fields; fills attribute placeholders and escapes snippet syntax; expands CSS properties and ignores unknown words.
- **`settings:editor.general.postfixCompletion` → stays partial.** New: `…::proofs39_zig_tab::the_zig_tab_lists_the_zig_templates` and `…::proofs39_java_var_row::the_java_tab_describes_var_as_introduce_variable_does`. The missing clause is unchanged.
- **`settings:editor.liveTemplates` → stays partial.** New: `…::proofs39_zig_group::the_page_lists_the_zig_templates`. The missing clause is unchanged.

## 5. Hypotheses in the notes, and which way each went

- A background tab's editor follows the document: **confirmed** (the two-file test reads editor a while b's tab is active).
- The watcher leaves a closed file's undo record alone after another program writes it: **confirmed** (the second Undo goes through).
- A `.java` file in the default mode starts no language server: **confirmed** (the server list is empty in all three Java tests).
- The lookup item `dependency` replaces the typed `dep`: **confirmed**.
- The newer-unsaved-typing case is refused by the backend and nothing is lost: **confirmed** by the new test.

## 6. Files

Edited, all under `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/`:

- `editor/intelligence.rs`, `settings_view/pages/editor_live_templates/tests.rs`, `settings_view/pages/editor_postfix_completion.rs`
- `templates/editor.rs`, `templates/emmet_model.rs`, `templates/expand.rs`, `templates/live_builtins.rs`, `templates/live_surrounders.rs`, `templates/mod.rs`, `templates/postfix_model.rs`, `templates/proofs39_completion_enter.rs`
- `workbench/editor_language.rs`, `workbench/editor_templates.rs`, `workbench/mod.rs`, `workbench/tests/proofs39_agent_undo.rs`, `workbench/tests/proofs39_java_var.rs`

Declared and unchanged: the other drafts the brief handed me. No new files. `proofs39_agent_undo_pending.rs` stays undeclared. Formatting was through `$S/fmt.sh`, on those files only.

## 7. Stale or false inputs

- HEAD is `4a26c3517`, not `52ba9ef7a`.
- The notes call the draft files untracked; they are tracked (`git ls-files` finds them, and `git diff` lists the ones I edited).
- The notes' anchors for slice B still match the tree, apart from `editor.rs`, `postfix_model.rs` and `proofs39_completion_enter.rs`, which needed a rustfmt reflow or a lint attribute.
- `$S/fmt.sh` leaves `/tmp/fmt_err_<pid>` files behind; some are from my runs, and I did not delete them.
