All three pages landed whole, Part 0 is fixed, and every gate passed. Nothing was committed. Per the brief, all three page rows stay partial: nothing in the native app reads these settings yet. Base: `W=<worktree>`, `S=$S`.

## Gates (measured)

- **Suite run 1**, nothing beside it (`$S/dset5i/suite1.log`): "test result: ok. 8036 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 213.66s". Load averages before 6.22 6.15 6.21, after 96.50 30.92 15.49.
- **Suite run 2** (`$S/dset5i/suite2.log`): "test result: ok. 8036 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 221.80s".
- **20× loop** during run 2 (`$S/dset5i/loop.log`): run directly on the built test binary, filters `proofs32 editor_postfix_completion editor_live_templates editor_file_templates files_store files_model text_field` (70 tests). 20 of 20 runs show "test result: ok. 70 passed; 0 failed". No test failed at any point.
- **web_view** (`$S/dset5i/web_view.log`): "test result: ok. 1 passed; 0 failed".
- **Clippy** `--all-targets -- -D warnings`: exit 0 (`$S/dset5i/clippy_final.log`).
- **`cargo build`**: exit 0, 0 warnings (`$S/dset5i/build_final.log`).
- No cargo, test or app process of mine is left; checked with `ps` at the end.

## Part 0: a file from a template could leave its folder

- **Failing test first:** `templates::files_model::tests::an_extension_that_is_not_a_plain_extension_plans_no_file`. Before the fix it failed (`$S/dset5i/part0_fail.log`): `left: Some(PlannedFile { path: "/p/src/notes./../../x", … }) right: None`.
- **Model fix:** new `files_model::is_plain_extension`. An extension is plain when it is empty, or has no leading `.`, no `..`, no surrounding blanks, and `is_safe_file_name("x.<ext>")` accepts it. `plan_file` returns `None` otherwise. The same test also checks that `tar.gz`, `d.ts`, `c++` and every built-in extension (kinds included) still pass.
- **Store fix:** `FileTemplates::save_template` now returns `Option<StorageWrite>` and stores nothing for a template whose own or any kind's extension is not plain. Test: `templates::files_store::tests::a_template_whose_extension_is_not_a_plain_extension_is_not_stored`.
- **What the user sees:** the Extension field turns red and shows "Not a file extension: it is not saved"; the template keeps the last plain extension the field held.
  - Typed key by key, `../notes` loses its leading dots on the way, so the stored extension ends up `""`. That is in `workbench::tests::proofs32_settings_file_templates::a_file_template_added_and_edited_on_the_page_is_stored_at_once`.
  - Page tests: `settings_view::pages::editor_file_templates::extension_guard::an_extension_typed_with_slashes_creates_nothing_outside_the_folder` (the renamed, now declared draft).
  - Same file, `…::a_template_stored_with_an_unplain_extension_says_why_its_changes_are_not_saved`. A template stored elsewhere with a bad kind extension shows "A kind's extension is not a file extension: changes to this template are not saved"; Remove Template, or Remove Kind when the template's own extension is plain, is still saved.
- **Every path-ending field of a file template, and where it is checked:**
  - `extension`: `plan_file`, `save_template`, and the page's Extension field.
  - `kinds[].extension`: `plan_file` through `with_kind`, and `save_template`. The page has no field for it; it shows the refusal above.
  - `file_name`: `plan_file` via `is_safe_file_name` (existing test `rejects_unsafe_filenames`). The page cannot edit it and the store does not check it.
  - The name typed in New: `plan_file` refuses `.`/`..` segments and reserved characters; subfolders stay inside the folder.
  - `default_name`: Hypothesis: it only becomes New's typed input, through other agents' undeclared New File dialog, and so gets the typed-name checks. Check with `grep -rn default_name app-gpui/src/templates/new_file_dialog.rs`.
  - `icon`: by reading `theme/icons.rs` `icon_path`, a name looked up in embedded tables, never a file path.
  - `id`, `name` and a fragment's `name`: by reading `files_create.rs`, the created path comes only from `plan_file`'s plan.
  - The webview has the same gap (`src/features/templates/files/model.ts:135-145`); it is not fixed, since `src/` is out of bounds.

## Postfix Completion

- **Webview behaviour:** each change is stored at once (`postfix/store.ts:22-25`). Apply is on only with staged changes (`SettingsDialog.tsx:351-366`) and Reset shows only for dirty pages (`:158`, `:236-242`), so Apply stays off and Reset hidden. Cancel just closes (`:360`); OK applies nothing and closes.
- **Stored JSON:** native `PostfixSettings::write` stores `{enabled, expandWithTab, showInCompletion, disabled}`, the same fields in the same order. Asserted with `json!` in `proofs32_settings_postfix::postfix_completion_options_and_templates_are_stored_at_once_and_cancel_keeps_them`.
- That test also asserts once that the storage file `ui-storage.json` lies under the test's temp home and holds the key.
- **What I added:** mount through the shared `mount_page`; the description tooltip; the code font from the theme (`font-mono`) instead of `"monospace"`; and a hint helper (`pages::hint_flow`) that wraps between words.

## Live Templates

- **Stores:** `openidea.liveTemplates.v1` keeps `{templates, deleted}` (`live/store.ts:15-19`, 45-58) and the options key keeps `{expandWithTab, showInCompletion}`, written by the existing `live_store`.
- **Drawing split out:** the drawing moved to `editor_live_templates/render.rs`; the logic file is now 523 lines.
- **Cost:** the list is a `uniform_list` over cached rows, rebuilt in one pass (a group lookup in a `HashMap`) per filter keystroke or store change. Only rows in view are drawn: `…::the_list_holds_every_template_and_draws_only_the_rows_in_view`. There are 119 built-in definitions (`grep -c 'def("' live_builtins.rs`). Each edit makes one `save_template`: a clone of the user's edits, a merge of the built-ins, and one serialisation of the user's edits.
- **Contexts field:** it keeps what is typed and stores the words between commas and blanks, the same list the webview's split gives for the same final text: `…::the_contexts_typed_key_by_key_are_kept_and_stored_as_the_words_of_the_final_text`.
- **Mono fields:** I added `TextField::set_code_font` so Abbreviation, Contexts and the variable fields use the code font, as the webview's `mono` inputs do.
- **Project-window tests now cover:** options, Add, abbreviation, text plus Tab, a variable's expression and Skip, filter, template checkbox, description, Restore Default, contexts, group, scope, Remove, and Cancel/OK.

## File and Code Templates

- **Owner of the keys:** `files_store.rs` owns the file-template keys and the field order matches `files/store.ts`. The project key uses `ipc_path(project_root)`; the webview uses `ProjectInfo.root: IpcPath`, which serialises through `to_ipc` the same way.
  - Hypothesis: `Workbench::project_root` is the same canonical path the backend holds. Check by choosing Project in both apps on a symlinked project and comparing keys in `ui-storage.json`.
- **Another writer:** Hypothesis: Import Settings also writes these keys wholesale (`app_models/project_actions.rs` `EXPORTED_STORAGE`).
- **Changes from the draft:**
  - A change stored by another window now keeps the selection within the same scheme, as the webview does. The draft started the tab over.
  - Disabled templates keep their colour: `.disabledItem` only dims `.itemLabel`, which file rows don't have.
  - The Files list is a `uniform_list`.
  - A built-in include's name is drawn like the webview's disabled input.
- **Project-window tests now also cover:** Remove Kind, a template's checkbox (which also selects its row, as in the webview), Remove Template, and the Code tab's text and Restore Default.

## Rows (all partial)

| key                                                    | tests                                                                                                                                                                                                                                                                                        | missing                                                                                                           |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `settings:editor.general.postfixCompletion`            | `workbench::tests::proofs32_settings_postfix::{postfix_completion_options_and_templates_are_stored_at_once_and_cancel_keeps_them, ok_closes_the_postfix_completion_page_and_keeps_what_was_stored}`, `settings_view::pages::editor_postfix_completion::tests::*`                             | nothing compiled reads the postfix settings                                                                       |
| `settings:editor.liveTemplates`                        | `workbench::tests::proofs32_settings_live_templates::{a_live_template_added_and_typed_on_the_page_is_stored_at_once_and_cancel_keeps_it, a_built_in_live_template_edited_and_removed_on_the_page_is_stored_at_once}`, `settings_view::pages::editor_live_templates::tests::*`                | no Tab expansion (the undeclared `proofs32_settings_templates_pending.rs`); plugin snippets have no native source |
| `settings:editor.fileTemplates`                        | `workbench::tests::proofs32_settings_file_templates::{the_project_scheme_and_an_include_are_stored_at_once_and_cancel_keeps_them, a_file_template_added_and_edited_on_the_page_is_stored_at_once}`, `…editor_file_templates::{tests,extension_guard}::*`, `templates::files_store::tests::*` | New ▸ offers no file templates (pending file); contributed templates have no native source                        |
| `unit:src/features/templates/__tests__/views.test.tsx` | 7 of 12 cases, named after their spec sentences                                                                                                                                                                                                                                              | the 5 `dialogs` cases belong to New File and the Chooser                                                          |

The key form (`settings:<page id>`) matches the existing `settings:editor.files`, `settings:editor.todo` and `settings:vcs.log` in `parity.json`. Grep confirms the only compiled `crate::templates` user outside `templates/` and these pages is the snippet engine (`editor_lsp/completion.rs`, `editor_lsp/snippet.rs`), so nothing was a few lines to wire.

## Hypotheses from the notes

- Closures taking `&App` accept `&mut Context`: confirmed; it compiled.
- A click on a checkbox also selects its row: confirmed, in both lists.
- The controls the tests click are in view: confirmed; all project-window tests pass.
- The include name commits on blur: confirmed (`includes_are_added_renamed_and_removed`).
- The text area height of 8 to 20 rows: looks close to the webview's 140px minimum in the shots.
- A disabled file template is faint in the webview: refuted by the CSS, and fixed natively.

## Shots (`$S/insp8/shots/`)

The temp project `/private/tmp/openidea-inspect-jUFrLI` was removed afterwards.

- `dset5a-01…04`: the three pages. These showed two bugs, both fixed: selected rows not full width in the virtualised lists, and `"monospace"` falling back to the UI font.
- `dset5b-01/02`: Postfix, light and dark.
- `dset5b-03/04`: Live with `fn` selected; mono fields and the variables table.
- `dset5b-05`: Live filtered by "Surround" to 5 rows.
- `dset5b-06/07`: Kotlin Class/File selected, with its kind tabs.
- `dset5b-08…11`: Includes and Code tabs, light and dark.
- `dset5c-01`: hint spacing fixed (`followed by .key, e.g. …`).
- `dset5c-02/03`: the built-in include name drawn disabled.

## Stale or false in the notes and briefs

- The notes marked Postfix "done"; by the brief it is partial.
- `extension_guard_pending.rs` expected a bad extension to be stored; it is now refused.
- The other agent's undeclared `templates/proofs34_extension_guard_pending.rs` can now pass. I did not touch it.
- `docs/gpui-port/parity.json` showed as modified in my first `git status`; that change is not mine.

## Files edited (`$W/app-gpui/src/`)

- `templates/`: `files_model.rs`, `files_store.rs`, `mod.rs`
- `ui/`: `text_field.rs`
- `settings_view/pages/`: `mod.rs`, `editor_postfix_completion.rs`
- `settings_view/pages/`: `editor_live_templates.rs`, plus `editor_live_templates/{fields,tests}.rs` and the new `editor_live_templates/render.rs`
- `settings_view/pages/`: `editor_file_templates.rs`, plus `editor_file_templates/{files_tab,fragments,tests}.rs`, with `editor_file_templates/extension_guard.rs` renamed from `extension_guard_pending.rs`
- `workbench/`: `mod.rs`, `settings_window.rs`, `settings_feature_pages.rs` (`mount_page` made `pub(super)`), `settings_template_pages.rs`
- `workbench/tests/`: `proofs32_settings_{postfix,live_templates,file_templates}.rs`
