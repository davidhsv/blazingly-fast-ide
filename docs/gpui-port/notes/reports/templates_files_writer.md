I wrote all three parts as 12 new, undeclared files plus the integration notes. Nothing has been compiled or run: per the brief I ran no cargo, edited no existing file and ran no git command that changes anything. "Done" in the table below means what I expect once the hunks land and the tests pass.

**Files** (worktree `app-gpui/src/…`):

| file                                            | lines | part                 |
| ----------------------------------------------- | ----- | -------------------- |
| `templates/new_file_dialog.rs`                  | 790   | 1                    |
| `templates/proofs34_new_file_dialog.rs`         | 335   | 1                    |
| `templates/proofs34_extension_guard_pending.rs` | 39    | 1 (leave undeclared) |
| `workbench/template_files.rs`                   | 391   | 1                    |
| `workbench/tests/proofs34_template_files.rs`    | 244   | 1                    |
| `workbench/template_actions.rs`                 | 207   | 2                    |
| `workbench/tests/proofs34_template_actions.rs`  | 251   | 2                    |
| `templates/proofs34_chooser_pending.rs`         | 88    | 2 (leave undeclared) |
| `templates/update_tag_dialog.rs`                | 200   | 3                    |
| `templates/proofs34_update_tag_dialog.rs`       | 69    | 3                    |
| `workbench/emmet.rs`                            | 86    | 3                    |
| `workbench/tests/proofs34_emmet.rs`             | 125   | 3                    |

The notes are at `$S/drafts/templates_files_integration.md` (411 lines): sections (a) to (f) as asked, plus (g) on where the brief differs.

**Parts:** all three are whole. Part 1 lands alone; Part 2 needs Part 1; Part 3 is independent. Not started: the `templates.chooser` overlay. It is the live templates' caret chooser in the uncompiled `chooser_view.rs`, not this brief's view.

**Rows I expect:**

| key                                                                                         | state                                    | proved by / missing                                                                                                         |
| ------------------------------------------------------------------------------------------- | ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `action:templates.newFromTemplate`                                                          | done                                     | three tests in `proofs34_template_actions` (list form, own-template form, extension+kind+name form), plus its New-popup row |
| `action:(dynamic) src/features/templates/index.ts:79`                                       | done                                     | `the_template_actions_and_rows_follow_the_templates_in_effect` runs the action, then checks the file on disk and its tab    |
| `point.registerMenuGroup:templates.new` / `templates.newFromTemplate`                       | done                                     | `the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog`                                                     |
| `point.registerOverlay:templates.newFile`                                                   | done                                     | the 9 tests in `proofs34_template_files`                                                                                    |
| `point.registerOverlay:templates.emmetUpdateTag`, `point.registerMenuGroup:templates.emmet` | done                                     | `proofs34_emmet`                                                                                                            |
| `action:emmet.updateTag`                                                                    | partial                                  | the editor menu row is checked to be there, but never clicked                                                               |
| `point.registerOverlay:templates.chooser`                                                   | not started                              | needs `chooser_view.rs` compiled                                                                                            |
| `unit:…views.test.tsx`                                                                      | partial: 3 of the 5 dialog/chooser cases | the 2 chooser cases are in the pending file                                                                                 |
| `unit:…pluginFiles.test.tsx`                                                                | done: 6 of 6                             | `proofs34_new_file_dialog`                                                                                                  |
| `unit:…files.test.ts`, `unit:…pluginPackages.test.ts`                                       | already done by name                     | no new tests                                                                                                                |
| `unit:…emmet.test.ts`                                                                       | unchanged, 4 of 7                        | the 3 cases need an Emmet expansion engine, out of scope                                                                    |

**What breaks when the hunks land:**

- **Existing tests:** registering the actions breaks three existing tests and one group-list test. The notes give a rewrite for each (2f, 2h): `editor_extras` `go_to_test_offers_create_new_test_only_with_the_file_templates`, `http_client` `new_http_request_creates_…`, `mod.rs` `the_tree_context_menu_offers_the_model_rows_and_creates_from_new`, and `context_menus` `every_menu_group_is_listed_once`.
- **Search tests:** any Find Action or Search Everywhere test that counts results for "new" may also change; I did not check those.
- **Dead code:** with Part 1 alone the build has no dead code, but only because I removed two setters (`set_file_templates`, `set_templates_and_actions`) that only tests called. Templates are now replaced in tests by setting the `FileTemplateSet` global and calling `sync_template_actions`.

**Hypotheses (H1–H12 in the notes, each with how to settle it):**

- Hypothesis: GPUI hands Enter to the key claim while an input method is composing, so declining the claim lets the IME confirm the text.
- Hypothesis: quiet Reformat (`explicit: false`) with no language server returns `Ok(false)` quickly and shows nothing.
- Hypothesis: Save All writes `# todo\n` unchanged.
- Hypothesis: `editorLangId` is `html` for `.html` and `vue` for `.vue`.
- Hypothesis: the Update Tag edit maps the caret as the webview's CodeMirror does (`Assoc::Before`).
- Hypothesis: one set of `templates.new.<id>` actions per process is enough; this breaks only once per-project schemes from `files_store` land.
- Hypothesis: the project's name is its folder's name.
- Hypothesis: `ops::project::roots` returns the same roots as the webview's `currentRoots()`.
- Hypothesis: `open_project_file` opens a file outside the project.
- Hypothesis: the webview `Dialog` presses its default button on Enter.
- Hypothesis: titles remembered with `tx_built` come out translated in Find Action.
- Hypothesis: a few constructs compile as written (Pixels `* 0.5`, a `let`-`else` with an or-pattern, `WeakEntity::update` on an async context).

**What turned out wrong in the brief:**

- `create.ts` creates exactly one file, so the "second file fails" case does not exist.
- There are no live-template variables in file creation, only the template's prompted variables.
- There is no overwrite question: the backend refuses an existing file ("<path> already exists") and the dialog shows that message.
- Restricted Mode is not special-cased: `fs_create_file` needs no trust, so creation works as in a trusted project.
- New from Template's template list is the first step of `templates.newFile`, not the `templates.chooser` overlay.
- `files.test.ts` and `pluginPackages.test.ts` were already covered by name.

**What turned out different in the native code:**

- `plan_file` lets an extension holding a path separator escape the folder, as the brief warned. I added my own check (`stays_in`) so nothing is created outside the folder, and the pending test fails until `plan_file` itself refuses it.
- Nothing natively follows created files, so the webview's created-file announcement (which offers to add the file to Git) is left out.
- The `templates.new` menu group has no rows today.
- `ActionItem` takes `&'static str` ids, so the dynamic ids go through the existing `static_action_id`.
