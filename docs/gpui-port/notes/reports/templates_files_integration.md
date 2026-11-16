# Integrating New from Template, the New File dialog and Emmet's Update Tag

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Every file below is new and undeclared; nothing existing was edited. Anchors are quoted from the files as they are now; there are no line numbers. Run `cargo fmt` after pasting: a few lines of the drafts run past 120 columns.

The work is in three groups, one per part. Part 1 compiles alone (its entry point `Workbench::open_new_file_dialog` is a `pub` method of a `pub` type, so `-D warnings` raises no dead code). Part 2 needs Part 1. Part 3 is independent of both.

New files:

| file                                                         | part | what                                                                                                  |
| ------------------------------------------------------------ | ---- | ----------------------------------------------------------------------------------------------------- |
| `app-gpui/src/templates/new_file_dialog.rs`                  | 1    | `NewFileDialog`: the template list step and the name step                                             |
| `app-gpui/src/templates/proofs34_new_file_dialog.rs`         | 1    | its view tests (views.test.tsx dialog cases, pluginFiles.test.tsx)                                    |
| `app-gpui/src/templates/proofs34_extension_guard_pending.rs` | 1    | NOT to declare yet: fails until `plan_file` refuses an extension holding a separator                  |
| `app-gpui/src/workbench/template_files.rs`                   | 1    | the dialog over the workbench, the package lookup, the creation, `FileTemplateSet`                    |
| `app-gpui/src/workbench/tests/proofs34_template_files.rs`    | 1    | end-to-end tests of the creation                                                                      |
| `app-gpui/src/workbench/template_actions.rs`                 | 2    | `templates.newFromTemplate`, `templates.new.<id>`, the `templates.new` rows                           |
| `app-gpui/src/workbench/tests/proofs34_template_actions.rs`  | 2    | end-to-end tests of the actions and the New popup, plus two unit tests                                |
| `app-gpui/src/templates/proofs34_chooser_pending.rs`         | 2    | NOT to declare yet: the two chooser cases of views.test.tsx, against the uncompiled `chooser_view.rs` |
| `app-gpui/src/templates/update_tag_dialog.rs`                | 3    | `UpdateTagDialog`                                                                                     |
| `app-gpui/src/templates/proofs34_update_tag_dialog.rs`       | 3    | its view tests                                                                                        |
| `app-gpui/src/workbench/emmet.rs`                            | 3    | `emmet.updateTag`                                                                                     |
| `app-gpui/src/workbench/tests/proofs34_emmet.rs`             | 3    | end-to-end tests                                                                                      |

## (a) Hunks, in the order to apply them

### Part 1: the New File dialog (`templates.newFile`)

**1a.** `app-gpui/src/templates/mod.rs`: after the line

```rust
pub mod live_surrounders;
```

add

```rust
pub mod new_file_dialog;
```

and at the end of the file, after `pub mod storage;`, add

```rust

#[cfg(test)]
mod proofs34_new_file_dialog;
```

**1b.** `app-gpui/src/workbench/mod.rs`, the module list at the top: after the line

```rust
mod structure_window;
```

add

```rust
mod template_files;
```

It only has to sit among the `mod` lines at the top; `mod structure_window;` is the nearest stable neighbour.

**1c.** `app-gpui/src/workbench/mod.rs`, inside `#[cfg(test)] mod tests {`: after the line

```rust
    mod proofs30_settings_run_targets;
```

add

```rust
    mod proofs34_template_files;
```

Nothing else changes for Part 1. The dialog uses the existing dialog slot (`open_vcs_dialog`, `place_vcs_dialog_at_top`, `vcs_dialog_view`, `close_vcs_dialog` in `branches.rs`). It waits for the new file's editor by polling, as `annotate.rs` does, so it needs no hook in the editor-opened code. It reuses `editor_language::{call, format_document, FormatRun, This}`, which are all `pub(super)`.

### Part 2: the actions and the New menu's groups

**2a.** `app-gpui/src/workbench/mod.rs`, the module list: after the `mod template_files;` line of 1b add

```rust
mod template_actions;
```

**2b.** `app-gpui/src/workbench/mod.rs`, in `pub fn register_actions(cx: &mut App)`: after the line

```rust
    windows::register_actions(cx);
```

add

```rust
    template_actions::register_actions(cx);
```

It must come before `let registry = cx.default_global::<ActionRegistry>();`, because it takes `cx` itself: it remembers the action titles in the text catalogue and keeps the registered ids in a global.

**2c.** `app-gpui/src/workbench/context_menus.rs`, `pub struct GroupFacts`: after

```rust
    /// The Spring group's rows for the menu being built: the generate actions at the caret.
    pub http_spring: Vec<GroupRow>,
```

add

```rust
    /// The file templates' New rows for the menu being built: one per enabled template but File.
    pub templates_new: Vec<GroupRow>,
```

**2d.** `app-gpui/src/workbench/context_menus.rs`, `fn group_facts`: after the line

```rust
            http_spring: if menu == ContextMenu::Editor { self.http_spring_rows(ctx, cx) } else { Vec::new() },
```

add

```rust
            templates_new: if menu == ContextMenu::New { self.templates_new_rows(ctx, cx) } else { Vec::new() },
```

**2e.** `app-gpui/src/workbench/context_menus.rs`, `MENU_GROUPS`: replace

```rust
    // One row per enabled file template.
    GroupDecl { id: "templates.new", menus: &[ContextMenu::New], order: 10, rows: None },
```

with

```rust
    // One row per enabled file template.
    GroupDecl {
        id: "templates.new",
        menus: &[ContextMenu::New],
        order: 10,
        rows: Some(super::template_actions::templates_new_group),
    },
```

**2f.** `app-gpui/src/workbench/context_menus.rs`, the test `every_menu_group_is_listed_once`: remove the line

```rust
                "templates.new",
```

from the expected list. The group has rows now.

**2g.** `app-gpui/src/workbench/mod.rs`, inside `mod tests {`: after the `mod proofs34_template_files;` line of 1c add

```rust
    mod proofs34_template_actions;
```

**2h.** Existing tests that assumed `templates.newFromTemplate` and `templates.new.file` are missing. Without these rewrites they fail. Each rewrite keeps what the test is about.

- `app-gpui/src/workbench/editor_extras.rs`, test `go_to_test_offers_create_new_test_only_with_the_file_templates`: it proves the branch without the action. Replace

  ```rust
          assert!(!cx.update(|_, cx| cx.global::<ActionRegistry>().contains("templates.newFromTemplate")));
  ```

  with

  ```rust
          // The branch without the file templates: their New File dialog is taken away.
          cx.update(|_, cx| cx.global_mut::<ActionRegistry>().unregister("templates.newFromTemplate"));
  ```

- `app-gpui/src/workbench/http_client/tests.rs`, test `new_http_request_creates_a_request_file_with_the_example_in_the_folder`. New › HTTP Request with a folder now asks for the name, as it does in the IDE (`http_client.rs` routes it through `templates.newFromTemplate` once that is registered). Replace the body after `let directory = f.path("api");` with

  ```rust
      run_action(cx, NEW_REQUEST, Some(json!({ "directory": directory })));
      cx.run_until_parked();
      assert!(f.workbench.read_with(cx, |w, _| w.new_file_dialog().is_some()), "the name is asked for");
      cx.simulate_input("requests");
      cx.simulate_keystrokes("enter");
      let first = format!("{directory}/requests.http");
      wait_long(cx, "the new request file", |cx| f.text_of(&first, cx).as_deref() == Some(NEW_REQUEST_TEMPLATE));
      assert!(Path::new(&first).is_file());
  ```

  Also change its doc comment to "New HTTP Request in a folder asks for the file's name and creates it there with the example request." The `requests_<n>.http` naming stays proven only for a request without a folder (the scratches).

- `app-gpui/src/workbench/mod.rs`, test `the_tree_context_menu_offers_the_model_rows_and_creates_from_new`. Outside the tree, File now runs `templates.new.file`, as the Project view's model says (`new_menu_items`, `outside_tree`). Replace

  ```rust
          run_action(cx, "project.newInThisDirectory", None);
          let popup = new_popup_of(&workbench, cx);
          cx.simulate_keystrokes("enter");
          enter_new_name(&popup, "more.rs", cx);
          settle(cx, |_| root.join("src/more.rs").is_file());
  ```

  with

  ```rust
          run_action(cx, "project.newInThisDirectory", None);
          new_popup_of(&workbench, cx);
          cx.simulate_keystrokes("enter");
          cx.run_until_parked();
          // Outside the tree File asks for the name in the New File dialog.
          assert!(workbench.read_with(cx, |w, _| w.new_file_dialog().is_some()));
          cx.simulate_input("more.rs");
          cx.simulate_keystrokes("enter");
          wait_long(cx, "more.rs is created", |_| root.join("src/more.rs").is_file());
  ```

### Part 3: Emmet's Update Tag

**3a.** `app-gpui/src/templates/mod.rs`: after `pub mod storage;` (before the `#[cfg(test)]` lines of 1a) add

```rust
pub mod update_tag_dialog;
```

and next to `mod proofs34_new_file_dialog;` add

```rust
#[cfg(test)]
mod proofs34_update_tag_dialog;
```

**3b.** `app-gpui/src/workbench/mod.rs`, the module list: after the line

```rust
pub mod empty_editor;
```

add

```rust
mod emmet;
```

**3c.** `app-gpui/src/workbench/mod.rs`, in `pub fn register_actions(cx: &mut App)`: after the line

```rust
    editor_extras::register_actions(registry);
```

add

```rust
    emmet::register_actions(registry);
```

**3d.** `app-gpui/src/workbench/mod.rs`, inside `mod tests {`: next to the other `proofs34_*` lines add

```rust
    mod proofs34_emmet;
```

The `templates.emmet` group already has its rows (`emmet_update_tag` in `context_menus.rs`). It shows them now because the action is registered.

### Pointing the drafts at the compiled code later

- `templates/files_store.rs` (uncompiled): what is needed is "the templates, includes and code templates in effect for the project at `root`". Once it compiles, replace the body of `workbench::template_files::templates_in_effect` with:

  ```rust
      let storage = cx.default_global::<crate::platform::storage::UiStorage>();
      let loaded = crate::templates::files_store::FileTemplates::load(root, &|key| storage.get(key).map(str::to_owned));
      FileTemplateSet {
          templates: loaded.templates().to_vec(),
          includes: loaded.includes().to_vec(),
          code_templates: loaded.code_templates().to_vec(),
      }
  ```

  Then make the Settings page, after each write, call `workbench::template_actions::sync_template_actions(&templates_in_effect(root, cx).templates, cx)` (it needs a `pub(crate)` re-export or a call through the workbench), so the actions follow the store. The `FileTemplateSet` global can stay as the tests' way to put templates in effect (`cx.set_global(set)` then `sync_template_actions`). One caveat: the registry is one per process and a Project scheme is per project. With two windows on different schemes, the last sync wins (see hypothesis H6).

- `templates/chooser_view.rs` (uncompiled): only the `templates.chooser` overlay (the caret chooser of Insert Live Template / Surround With) needs it. New from Template's template list is the first step of `NewFileDialog`, so it does not. Once `pub mod chooser_view;` builds, declare `templates/proofs34_chooser_pending.rs` as `#[cfg(test)] mod proofs34_chooser;`, renamed without `_pending`.
- `plan_file`'s extension guard: once it lands, declare `templates/proofs34_extension_guard_pending.rs` as `#[cfg(test)] mod proofs34_extension_guard;`. The workbench refuses such a plan on its own already (`template_files::stays_in`).

## (b) Parity rows

| key                                                            | expected                                                                                          | proving test(s)                                                                                                                                                                                                                                                                                                                                                                                                                                                           | missing for done                                                                                                               |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `action:templates.newFromTemplate`                             | done                                                                                              | `workbench::tests::proofs34_template_actions::new_from_template_lists_the_enabled_templates_and_creates_the_chosen_one`, `…::new_from_template_with_a_template_of_its_own_names_a_file_of_it`, `…::new_from_template_with_an_extension_opens_that_templates_dialog_with_the_kind_and_the_name`, the menu row in `…::the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog` (no shortcut exists)                                                           |                                                                                                                                |
| `action:(dynamic) src/features/templates/index.ts:79`          | done                                                                                              | `…::the_template_actions_and_rows_follow_the_templates_in_effect` (runs `templates.new.notes` through the registry, types a name, asserts the file's text on disk and its tab), `…::a_template_action_without_an_argument_uses_the_active_files_folder`, `…::the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog`, unit `…::the_template_actions_follow_the_templates_given`                                                                            | the store following the Settings page needs `files_store` (above)                                                              |
| `point.registerMenuGroup:templates.new`                        | done                                                                                              | `…::the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog`, `…::the_template_actions_and_rows_follow_the_templates_in_effect`                                                                                                                                                                                                                                                                                                                             |                                                                                                                                |
| `point.registerMenuGroup:templates.newFromTemplate`            | done                                                                                              | `…::the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog` (From Template… last, chosen, opens the list for the folder)                                                                                                                                                                                                                                                                                                                                   |                                                                                                                                |
| `point.registerOverlay:templates.newFile`                      | done                                                                                              | `workbench::tests::proofs34_template_files::*` (9 tests, the Go and PHP package lookups included) and the Part 2 tests                                                                                                                                                                                                                                                                                                                                                    |                                                                                                                                |
| `point.registerOverlay:templates.chooser`                      | not started (not this brief's view)                                                               |                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | `chooser_view.rs` compiled and shown by Surround With / Insert Live Template                                                   |
| `point.registerOverlay:templates.emmetUpdateTag`               | done                                                                                              | `workbench::tests::proofs34_emmet::update_tag_with_emmet_renames_the_element_and_sets_its_attributes`, `…::escape_closes_the_update_tag_prompt_without_editing`, `templates::proofs34_update_tag_dialog::*`                                                                                                                                                                                                                                                               |                                                                                                                                |
| `action:emmet.updateTag`                                       | partial                                                                                           | `proofs34_emmet::update_tag_with_emmet_renames_the_element_and_sets_its_attributes`, `…::update_tag_is_off_outside_markup_and_a_caret_outside_any_tag_is_told`, `…::an_abbreviation_of_several_elements_leaves_the_tag_and_warns`                                                                                                                                                                                                                                         | the row of the editor's right-click menu is asserted present, not chosen with the mouse                                        |
| `point.registerMenuGroup:templates.emmet`                      | done                                                                                              | `proofs34_emmet::the_editor_menu_offers_update_tag_for_markup_files_only`                                                                                                                                                                                                                                                                                                                                                                                                 |                                                                                                                                |
| `unit:src/features/templates/__tests__/views.test.tsx`         | partial: 3 of the 5 dialog and chooser cases                                                      | `templates::proofs34_new_file_dialog::shows_the_template_chooser_then_the_name_step`, `…::prefills_fixed_names_and_follows_the_kinds_extension`, `…::names_an_ad_hoc_template_from_another_feature`; pending: `templates::proofs34_chooser_pending::chooser_filters_by_typing_and_picks_with_enter`, `…::chooser_picks_by_mnemonic_and_skips_section_rows`                                                                                                                | the 2 chooser cases (draft `chooser_view.rs`); the file's other 7 cases (Settings pages, plugin snippets) are not this brief's |
| `unit:src/features/templates/__tests__/pluginFiles.test.tsx`   | done: 6 of 6                                                                                      | `templates::proofs34_new_file_dialog::creates_a_fixed_dotfile_without_asking_for_a_name`, `…::asks_all_variables_together_validates_them_and_creates_only_on_submit`, `…::cancels_fixed_name_variables_without_creating_anything`, `…::preserves_plugin_prompts_when_they_collide_with_the_ide_text_catalog`, `…::does_not_submit_while_confirming_ime_text` (the `it.each`: both fields, in one loop), `…::does_not_create_when_the_variables_dialog_closes_with_escape` |                                                                                                                                |
| `unit:src/features/templates/__tests__/files.test.ts`          | done already: the cases have native tests by name in `templates::files_model::tests`; no new test |                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |                                                                                                                                |
| `unit:src/features/templates/__tests__/pluginPackages.test.ts` | done already: 7 of 7 by name in `templates::files_create::tests`; no new test                     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |                                                                                                                                |
| `unit:src/features/templates/__tests__/emmet.test.ts`          | partial, unchanged: 4 of 7 (`templates::emmet_model::tests`)                                      |                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | the 3 expansion cases need an `EmmetEngine` (Tab expansion is not this brief's)                                                |

IPC commands these actions stand for:

- `fs_create_file`: `ide_host::ops::fs::create_file(host, window_label, path)`, the same call with the same arguments.
- `fs_read_dir` and `fs_read_text` (`ops::fs::read_dir`, `ops::fs::read_text`): the Go and PHP package lookups.
- `project_roots` (`ops::project::roots`): the JVM source roots.
- `lsp_format`, through `format_document` with `explicit: false, selection_only: false`, as the IDE's `reformat(path)` service runs it.
- `file.save`: run as the action of the same id.

Why the template list is not a compiled list popup: `ListPopup` (`editor_lsp/intentions_view.rs`) filters by speed search and closes on its choice, and `PopupMenu` (`ui/popup_menu.rs`) is a menu at a point. The template list has no filter, sits in a titled dialog under the folder line, and turns into the name step in place. So it is drawn by `NewFileDialog` itself, about 40 lines.

## (c) The behaviours, with their source

Built:

- `src/features/templates/index.ts:31-48`: Update Tag with Emmet. "No tag at the caret" is Info and "Cannot update the tag" is a Warning with the body `'<abbr>' does not describe a single element.`, both under key `emmet-update-tag`. The editor gets the keyboard back first. Built in `workbench/emmet.rs`.
- `index.ts:61-65` `isTemplate` and `index.ts:67-71` `templateForExtension`: `template_actions::ad_hoc_template` (a missing `id` / `icon` defaults to "") and `template_for_extension`.
- `index.ts:73-76` `directoryOf`, with `files/create.ts:110-113` `defaultDirectory`: `Workbench::template_directory`. It takes the argument's folder, else the active file's folder, else the root. An empty folder does nothing.
- `index.ts:78-93` `newFileAction`: id, title (`New Empty File…` / `New <name>…`), category, icon, `when: projectOpen`, keywords. All in `new_file_action`.
- `index.ts:96-124` the New groups: `templates_new_rows` (enabled templates but `file`, each with `{directory}` of the menu's folder and the template's name), and the existing `new_from_template` group function.
- `index.ts:126-140` the actions following the store: `sync_template_actions` unregisters and re-registers. What tells it the store changed is the `files_store` hunk above.
- `index.ts:177-179` the overlays `templates.newFile` and `templates.emmetUpdateTag`.
- `index.ts:180-186` `templates.emmet`: already in `context_menus.rs`, live now.
- `index.ts:225-232` `emmet.updateTag` with its `when`: taken from the manifest.
- `index.ts:233-253` `templates.newFromTemplate` with every argument form.
- `files/NewFileDialog.tsx:34-60`: an explicit template, else one looked up by id among all templates (disabled ones too), else the list of the enabled ones.
- `NewFileDialog.tsx:62-109`, the list step: title "New", width 320, near the top, the folder line, icon and name rows, hover selects, the arrows go round, Home / End / Page keys, Enter or a click picks.
- `NewFileDialog.tsx:122-147`: the initial name (`fileName ?? name ?? defaultName`), the variables, the initial kind, the effective template and the plan.
- `NewFileDialog.tsx:149-158`: the package for the hint (`PackageWanted` / `set_package`, stale answers dropped).
- `NewFileDialog.tsx:160-199`: the submit checks with "Invalid name", "Enter a name" and "Enter a value for each variable". It runs once (busy). A failure before the file exists shows the backend's message in the hint.
- `NewFileDialog.tsx:201-206`: auto-create for an exact file name without variables (`NewFileDialog::start`).
- `NewFileDialog.tsx:208-226`: the keys. The IME guard checks the fields' marked text. The arrows change the kind only with several kinds. Enter submits.
- `NewFileDialog.tsx:228-262`: the hint path, the title `New <name>`, width 380, Create and Cancel only with variables, the fields read-only while busy, the placeholder `Name (.<ext>)` or "File name with extension", the red border on error.
- `NewFileDialog.tsx:263-280`: the variable prompts, shown as written (not translated).
- `NewFileDialog.tsx:281-307`: the kind list (a press selects, a double click creates), and the hint with ` · package X` or ` · namespace X`.
- `files/create.ts:28-69`: the Go sibling package, the JVM package roots and the composer.json walk (`template_files::finish_lookup`, `project_roots`).
- `create.ts:120-185`:
  - plan, package, render (with the project's name, user, includes and code templates), then the cancelled check;
  - `fsCreateFile`, after which the dialog closes and the file opens;
  - a wait of about 4 s for its editor, else the "Created <file>" warning;
  - only an empty document is filled, with the caret at `$END$` and the focus;
  - Reformat when on, keeping the caret;
  - `file.save`.

  It creates exactly one file, so there is no second file that could fail. Missing folders are created by the backend's `create_file`.

- `create.ts` and the webview's Restricted Mode: the webview checks nothing, and `ops::fs::create_file` does not `require_trust`, so it works the same way (tested).
- `emmet/UpdateTagDialog.tsx:21-56`: title, width 420, the label `Abbreviation for <tag>, e.g.` followed by the example in the code font, OK disabled while blank, Cancel.
- `emmet/editor.ts:26-31`: the tag is found again at the caret when applying.

Added, not in the webview: `template_files::stays_in` refuses a plan whose path leaves the folder New was used in. The brief requires it, and `plan_file` lets an extension with a separator through.

Left out:

- `create.ts:148` `notifyFileOperation({ kind: "created" })`: in the IDE this asks whether to add the file to Git. Nothing in the native window follows explicit file operations (`vcs_ops::confirmations` is a model nobody calls), so nothing is announced. The tree learns of the file from the watcher.
- `NewFileDialog.tsx:186-194`, the "Could not fill the new file" notification: once the file exists, every native step either reports itself (opening, saving) or can only fail because the window went away. No error is left to show there.
- `popup/Chooser.tsx` and its store (overlay `templates.chooser`): the Surround With draft's.
- `files/store.ts` (schemes, customisations, contributed templates): the uncompiled `files_store.rs`. The built-ins stand in for it until the hunk above.

## (d) Hypotheses (not checked by compiling or running)

- H1 Hypothesis: in GPUI on macOS, Enter pressed while an input method composes reaches `intercept_keystrokes` with the field's marked text still set, so declining the claim lets the IME confirm. Settle it with `does_not_submit_while_confirming_ime_text` and a manual check with a Chinese IME in the real window.
- H2 Hypothesis: `format_document` with `explicit: false` and no language server (Light mode, tests) returns `Ok(false)` quickly and shows nothing. Settle it with `a_java_class_gets_the_package_of_the_folder_its_name_puts_it_in` and `new_from_template_with_a_template_of_its_own_names_a_file_of_it`, watching for a hint or a notification.
- H3 Hypothesis: Save All writes `# todo\n` unchanged (no trailing-whitespace or final-newline rule in the test settings). Settle it with `a_file_from_a_template_is_created_opened_with_the_caret_at_its_end_marker_and_saved`.
- H4 Hypothesis: the language id of `.html` is `html` and that of `.vue` is `vue` in `editorLangId`. Settle it with the waits in `proofs34_emmet`.
- H5 Hypothesis: CodeMirror maps the selections of the Update Tag edit with `assoc = -1`, hence `Assoc::Before`. Settle it by comparing a caret inside the start tag in both apps.
- H6 Hypothesis: one set of `templates.new.<id>` actions per process is enough. The registry is global, and a Project scheme differs per window only once `files_store` lands. Settle it with two windows on two schemes after that hunk.
- H7 Hypothesis: the project's name for `$PROJECT_NAME$` is its folder's name (the backend's `ProjectInfo.name` in the IDE). Settle it with a project opened under a different display name.
- H8 Hypothesis: `ops::project::roots(..).roots` are the roots the IDE's `currentRoots()` gives. Settle it with a Maven project with a marked source root.
- H9 Hypothesis: `open_project_file` opens a file outside the project in a project window. Settle it with `a_folder_outside_the_project_gets_the_file_where_new_was_used`.
- H10 Hypothesis: the webview `Dialog` presses its default button on Enter, which the Update Tag prompt copies. Settle it by pressing Enter in the IDE's prompt.
- H11 Hypothesis: `I18n::tx_built(&["New ", "…"], …)` remembered at registration makes Find Action show `New {name}…` translated. Settle it with zh in the action list.
- H12 Hypothesis: these compile as written, though not compiled:
  - `(theme::length + theme::length) * 0.5` (`Pixels: Mul<f32>`);
  - the `let (A | B) = &self.content else` or-pattern;
  - `WeakEntity::update` with an `AsyncWindowContext`;
  - `Option<Entity<_>> == Option<Entity<_>>`;
  - `<[(String, bool); 2]>::try_from(Vec)`.

## (e) Tests most likely to fail first

- The three existing tests in 2h, unless rewritten. Also any Find Action / Search Everywhere test that counts the results for "new" or "file" now finds the `New <template>…` actions. Grep for such counts.
- `a_java_class_gets_the_package_of_the_folder_its_name_puts_it_in`: it runs Reformat on a Java file (H2), and the hint wait depends on `ops::project::roots` in a project without a build file.
- `new_from_template_with_a_template_of_its_own_names_a_file_of_it`: Reformat on an `.http` file (H2).
- `does_not_submit_while_confirming_ime_text` (H1). If simulated keystrokes bypass the claim while the text is marked, the first Enter creates.
- `a_folder_that_cannot_be_written_reports_the_failure_and_creates_nothing` fails when the tests run as root, since the permissions are ignored.
- `the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog`: the popup filter "Rust File" may also match a keyword row. Enter takes the first row.
- `prefills_fixed_names_and_follows_the_kinds_extension` and the other view tests open two windows in one test. The key interceptor is installed once (guarded on `Keymap`).

## (f) What a look at the real window must check

- New › (Project view popup):
  - the template rows with their icons (from the actions' `icon`) after File / Directory;
  - HTTP Request and Scratch File in their groups;
  - From Template… last after a separator;
  - typing filters the template rows, including by extension keyword (`css` finds Stylesheet).
- The New File dialog:
  - sits a tenth down the window, centred, with no dimming;
  - the list step is 320 wide with a muted folder line, and the name step is 380 wide;
  - the kind rows: selected background `bg-selected`, hover `bg-hover`, the template's 14px icon;
  - the hint line is small and subtle, and red with the error; the field border turns red on error;
  - a template with variables has its inset form with Cancel / Create;
  - Create is greyed out while the file is created.
- After Enter: the dialog closes before the tab opens, the caret blinks at `$END$`, and the tab is not marked modified once saved.
- Update Tag with Emmet:
  - 420 wide, the example in the code font;
  - OK greyed out while the field is blank;
  - the right-click menu of an HTML editor has the row, and that of a Rust editor does not.
- With a zh locale: `新建 Rust File`, the placeholder `名称 (.rs)`, and the buttons translated while a plugin's prompt stays as written.

## (g) Where the brief and the native code differ from what the webview does

- `create.ts` creates exactly one file per Enter. The "several files, second fails" case does not exist.
- File templates have only prompted variables (`template.variables`, asked in the dialog). There are no live-template variables in file creation.
- There is no overwrite question. The backend's `create_file` refuses an existing file (`AlreadyExists`, "<path> already exists"), and the dialog shows that message.
- Restricted Mode is not special-cased. `fs_create_file` does not require trust, so the file is created and filled as in a trusted project.
- New from Template's template list is the first step of the New File dialog (overlay `templates.newFile`). Overlay `templates.chooser` is the caret chooser of the live templates, which `chooser_view.rs` drafts.
- `files.test.ts` and `pluginPackages.test.ts` already have every case as a native model test by name. Nothing was added for them.
- Native code: `context_menus.rs` declares `templates.new` with `rows: None`, and its test lists the group as missing. `http_client.rs`, `editor_language.rs` and the Project view's New model already route through `templates.newFromTemplate` / `templates.new.file` once these are registered. Registering them therefore changes three existing tests (2h).
