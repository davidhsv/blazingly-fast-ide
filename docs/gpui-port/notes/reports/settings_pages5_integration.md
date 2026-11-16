# Settings pages 5: Postfix Completion, Live Templates, File and Code Templates

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Nothing here was compiled. All new files are untracked. No existing file was edited.

## Status per page

- `editor.general.postfixCompletion`: WHOLE (view, wiring, view tests, end-to-end tests).
- `editor.liveTemplates`: WHOLE (view, wiring, view tests, end-to-end tests). Plugin snippets have no native source yet: the view takes them through `set_contributed_snippets` and the case is proven on the view only.
- `editor.fileTemplates`: WHOLE (a new store model `templates/files_store.rs`, the view, wiring, view tests, end-to-end tests). Plugin-contributed file templates have no native source: the view takes them through `set_contributed_templates`, untested. One safety gap found in existing code (see (d) and group 3 hunk 3): an extension with slashes escapes the folder; its test is pending on that hunk.

What the dialog draws for these ids today (measured by reading `settings_view/dialog.rs:944-961` and `settings_model/builtin_pages/features.rs:51-60`): their specs have no sections and no view is set for them, so each page shows the "not available" line (`UNAVAILABLE`).

Wrong premise in the brief (measured by grep): nothing in `workbench/`, `editor/` or `project_view/` reads the live templates, the postfix settings or the file templates. The only users of `crate::templates` outside `templates/` are `editor_lsp/completion.rs` and `editor_lsp/snippet.rs`, which use `templates::snippet` (the snippet engine) only. No action `templates.*` is registered (`workbench/editor_extras.rs:1334` asserts `templates.newFromTemplate` is absent; `project_view/new_popup.rs` lists template rows only as contributed `templates.new.*` actions, none registered). So "edit a live template on the page, then expand its abbreviation in an editor" and "add a file template, then see it offered by New" cannot be proven: those two tests are in `app-gpui/src/workbench/tests/proofs32_settings_templates_pending.rs`, not declared. The pages read and write the UI storage under the same keys the webview uses, so the feature that later expands templates or offers New must read `templates::live_store::LiveTemplates::load(storage.get(LIVE_TEMPLATES_KEY))` and friends to see what the pages wrote.

## Files written

`editor_live_templates.rs` is 852 lines, over the ~800 the brief asks for; its render helpers (`side`, `item`, `editor`, `labelled`, `hint`) can move to a child `editor_live_templates/render.rs` without other changes.

- `app-gpui/src/settings_view/pages/editor_postfix_completion.rs`: `PAGE`, `LABELS`, `stored_postfix_settings(cx)`; `PostfixPageView::new(Binding, cx)`, `settings()`, `group()`, `toggle_enabled`, `toggle_expand_with_tab`, `toggle_show_in_completion`, `toggle_template(id)`, `select_group(prefix)`, `lines(cx)`; emits `ControlEvent` (never, in practice: it has no field). Selectors `settings-postfix-enabled`, `settings-postfix-expandWithTab`, `settings-postfix-showInCompletion`, `settings-postfix-group-{ts|rust|py|go|java|kt}`, `settings-postfix-template-{id}`. 3 view tests.
- `app-gpui/src/settings_view/pages/editor_live_templates.rs` + `editor_live_templates/fields.rs` + `editor_live_templates/tests.rs`: `PAGE`, `LABELS`, `group_templates(templates, filter)`, `parse_contexts`, `text_variables`, `FieldKind`; `LiveTemplatesPageView::new(Binding, cx)`, `store()`, `options()`, `selected()`, `filter_field()`, `editor_field(&FieldKind)`, `body_area()`, `can_remove()`, `set_contributed_snippets(Vec<ContributedSnippet>)`, `select(id)`, `toggle_enabled(id)`, `add`, `remove`, `restore_default`, `toggle_scope`, `toggle_skip(name)`, `toggle_expand_with_tab`, `toggle_show_in_completion`, `tree_lines(cx)`, `editor_lines(cx)`; emits `ControlEvent::{Submit, Cancel}` from Enter / Escape in its fields (Escape only in the template text, where Enter is a line break). Selectors `settings-live-expandWithTab`, `settings-live-showInCompletion`, `settings-live-add`, `settings-live-remove`, `settings-live-filter`, `settings-live-item-{id}`, `settings-live-enabled-{id}`, `settings-live-abbreviation`, `settings-live-description`, `settings-live-group`, `settings-live-body`, `settings-live-contexts`, `settings-live-scope`, `settings-live-skip-{NAME}`, `settings-live-restore`. 1 model test in `fields.rs`, 7 tests in `tests.rs`.
- `app-gpui/src/templates/files_store.rs` (529 lines): `FILE_TEMPLATES_KEY`, `scheme_key(root)`, `project_key(root)`, `TemplateScheme`, `Ordered<T>`, `FileTemplateCustomizations::parse`, `is_builtin_include`, `merge_file_templates`, `merge_includes`, `merge_code_templates`, `FileTemplates::{load, reload, storage_key, scheme, root, templates, includes, code_templates, custom, set_contributed, is_base_template, save_template, remove_template, reset_template, is_modified_template, save_include, remove_include, reset_include, is_modified_include, save_code_template, reset_code_template, is_modified_code_template, set_scheme}`: the webview's `files/store.ts` (read whole). 3 model tests.
- `app-gpui/src/settings_view/pages/editor_file_templates.rs` (612) + `editor_file_templates/files_tab.rs` (509) + `editor_file_templates/fragments.rs` (389) + `editor_file_templates/tests.rs` (205) + `editor_file_templates/extension_guard_pending.rs` (35, not declared): `PAGE`, `LABELS`, `TemplateTab`, `NAME_STYLES`, `FileField`; `FileTemplatesPageView::new(Binding, root: Option<String>, cx)`, `store()`, `tab()`, `lines(cx)`, `select_tab`, `set_scheme`; Files: `selected_file_template()`, `file_field(FileField)`, `file_body()`, `set_contributed_templates`, `select_template`, `toggle_template_enabled`, `add_template`, `remove_template`, `restore_template`, `set_name_style`, `select_kind`, `add_kind`, `remove_kind`, `toggle_reformat`; Includes: `selected_include_name()`, `include_name_field()`, `include_body()`, `select_include`, `add_include`, `remove_include`, `restore_include`; Code: `code_body()`, `select_code`, `restore_code`. Emits `ControlEvent::{Submit, Cancel}`. Selectors `settings-filetemplates-scheme`, `settings-filetemplates-scheme-{default|project}`, `settings-filetemplates-tab-{files|includes|code}`, `settings-filetemplates-add`, `settings-filetemplates-remove`, `settings-filetemplates-item-{id}`, `settings-filetemplates-enabled-{id}`, `settings-filetemplates-name`, `settings-filetemplates-extension`, `settings-filetemplates-style`, `settings-filetemplates-style-{asis|snake|pascal|kebab}`, `settings-filetemplates-kind-{i}`, `settings-filetemplates-add-kind`, `settings-filetemplates-remove-kind`, `settings-filetemplates-kind-label`, `settings-filetemplates-body`, `settings-filetemplates-reformat`, `settings-filetemplates-restore`, `settings-includes-{add|remove|name|body|restore}`, `settings-includes-item-{name}`, `settings-code-item-{name}`, `settings-code-body`, `settings-code-restore`. 1 model test in `fragments.rs`, 7 tests in `tests.rs`.
- `app-gpui/src/workbench/tests/proofs32_settings_file_templates.rs`: 2 end-to-end tests.
- `app-gpui/src/workbench/tests/proofs32_settings_templates_pending.rs`: 2 end-to-end tests, not declared (see the premise above).
- `app-gpui/src/workbench/settings_template_pages.rs`: `Workbench::mount_template_settings_pages(&dialog, cx) -> Vec<Subscription>` and `mount` (page view + search labels + OK/Cancel answered).
- `app-gpui/src/workbench/tests/proofs32_settings_templates_fixtures.rs`: `template_project`, `open_template_page`, `stored_template_json`, `nothing_staged`, `type_into`, `click_template`.
- `app-gpui/src/workbench/tests/proofs32_settings_postfix.rs`: 2 end-to-end tests.
- `app-gpui/src/workbench/tests/proofs32_settings_live_templates.rs`: 2 end-to-end tests.

## (a) Hunks, in order

### Group 0 (shared; apply first)

1. `app-gpui/src/workbench/mod.rs`, module list. Anchor (exact):

```
mod services_window;
mod settings_window;
```

Replace with:

```
mod services_window;
mod settings_template_pages;
mod settings_window;
```

(If the previous writer's `mod settings_feature_pages;` lands first, it sits between `services_window` and `settings_template_pages`.)

2. `app-gpui/src/workbench/settings_window.rs`, in `open_settings`. Anchor (exact):

```
        subscriptions.extend(crate::settings_view::custom_controls::install(&dialog, install, window, cx));
```

Add right after it:

```
        subscriptions.extend(self.mount_template_settings_pages(&dialog, cx));
```

3. `app-gpui/src/workbench/mod.rs`, inside `mod tests`. Anchor (exact):

```
    mod proofs26_save_all;
    mod proofs3_context;
```

Replace with:

```
    mod proofs26_save_all;
    mod proofs32_settings_file_templates;
    mod proofs32_settings_live_templates;
    mod proofs32_settings_postfix;
    mod proofs32_settings_templates_fixtures;
    mod proofs3_context;
```

(Do NOT declare `proofs32_settings_templates_pending.rs`.)

`settings_template_pages.rs` names every page module: to land one page at a time, delete the other pages' `let …_binding`, `cx.new`, `mount(…)` lines and `use` lines from it and their `mod proofs32_settings_*` line until their group lands.

### Group 1: Postfix Completion

`app-gpui/src/settings_view/pages/mod.rs`. Anchor (exact):

```
pub mod editor_files;
pub mod editor_font;
pub mod font_picker;
```

Replace with (this already carries groups 2 and 3; drop the lines of a group not landed):

```
pub mod editor_file_templates;
pub mod editor_files;
pub mod editor_font;
pub mod editor_live_templates;
pub mod editor_postfix_completion;
pub mod font_picker;
```

### Group 2: Live Templates

Only the `pub mod editor_live_templates;` line above. Its child modules (`fields`, `tests`) are declared inside the file.

### Group 3: File and Code Templates

1. The `pub mod editor_file_templates;` line above (its child modules `files_tab`, `fragments`, `tests` are declared inside the file; `extension_guard_pending` is not).

2. `app-gpui/src/templates/mod.rs`. Anchor (exact):

```
pub mod files_create;
pub mod files_model;
```

Replace with:

```
pub mod files_create;
pub mod files_model;
pub mod files_store;
```

3. (Safety fix in existing code; recommended, and the pending test depends on it.) `app-gpui/src/templates/files_model.rs`, in `plan_file`. Anchor (exact):

```
    let ext = if template.extension.is_empty() { String::new() } else { format!(".{}", template.extension) };
```

Add right after it:

```
    // The extension is part of the file's name: one that names another folder, or has a character no
    // file name can have, would create the file outside `directory` or not at all.
    if template.extension.contains(['/', '\\', '<', '>', ':', '"', '|', '?', '*']) {
        return None;
    }
```

`plan_creation` (`files_create.rs:198-216`) calls `plan_file` on `with_kind(template, Some(kind))`, so a kind's own extension goes through the same check. After applying it, rename `settings_view/pages/editor_file_templates/extension_guard_pending.rs` to `extension_guard.rs` and declare it in `editor_file_templates.rs` next to `mod tests;` with `#[cfg(test)] mod extension_guard;`. The webview has the same gap (`src/features/templates/files/model.ts:135-145` appends the extension unchecked); that is for a separate change there.

## (b) Rows

| key                                                                                                                                           | expected         | proving tests                                                                                                                                                                                                                                                                                                                                                                                                  | missing                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:editor.general.postfixCompletion` (new key; neighbours are `settings:editor.files`, `settings:build.cmake`, `settings:build.maven`) | done             | `workbench::tests::proofs32_settings_postfix::postfix_completion_options_and_templates_are_stored_at_once_and_cancel_keeps_them`, `workbench::tests::proofs32_settings_postfix::ok_closes_the_postfix_completion_page_and_keeps_what_was_stored`, `settings_view::pages::editor_postfix_completion::tests`                                                                                                     | nothing on the page; that the editor follows the settings is the postfix feature's row, not the page's (nothing reads them natively)                                                                                                                                                                                                                                                                                                          |
| `settings:editor.liveTemplates` (new key)                                                                                                     | partial          | `workbench::tests::proofs32_settings_live_templates::a_live_template_added_and_typed_on_the_page_is_stored_at_once_and_cancel_keeps_it`, `workbench::tests::proofs32_settings_live_templates::a_built_in_live_template_edited_and_removed_on_the_page_is_stored_at_once`, `settings_view::pages::editor_live_templates::tests`, `settings_view::pages::editor_live_templates::fields::tests`                   | `workbench::tests::proofs32_settings_templates_pending::a_live_template_added_on_the_page_expands_in_an_editor_of_the_window` (pending: no native Tab expansion of live templates); plugin snippets reach the page only through `set_contributed_snippets` (no native source of contributed snippets exists)                                                                                                                                  |
| `settings:editor.fileTemplates` (new key)                                                                                                     | partial          | `workbench::tests::proofs32_settings_file_templates::the_project_scheme_and_an_include_are_stored_at_once_and_cancel_keeps_them`, `workbench::tests::proofs32_settings_file_templates::a_file_template_added_and_edited_on_the_page_is_stored_at_once`, `settings_view::pages::editor_file_templates::tests`, `settings_view::pages::editor_file_templates::fragments::tests`, `templates::files_store::tests` | `workbench::tests::proofs32_settings_templates_pending::a_file_template_added_on_the_page_is_offered_by_new` (pending: New ▸ lists no file templates natively); `settings_view::pages::editor_file_templates::extension_guard_pending::an_extension_typed_with_slashes_creates_nothing_outside_the_folder` (pending on group 3 hunk 3); contributed file templates reach the page only through `set_contributed_templates` (no native source) |
| `unit:src/features/templates/__tests__/views.test.tsx` (12 cases)                                                                             | partial, 7 of 12 | see the per-case list below                                                                                                                                                                                                                                                                                                                                                                                    | the 5 `dialogs` cases (New File dialog, Chooser) are not these pages                                                                                                                                                                                                                                                                                                                                                                          |

`views.test.tsx` cases, with their native test:

1. settings pages › groups and filters live templates → `settings_view::pages::editor_live_templates::tests::groups_and_filters_live_templates`
2. settings pages › edits a live template → `settings_view::pages::editor_live_templates::tests::edits_a_live_template` (the native test types the filter first: the list is clipped and `psvm` is far below)
3. settings pages › toggles postfix templates → `settings_view::pages::editor_postfix_completion::tests::toggles_postfix_templates`
4. settings pages › renders file templates → `settings_view::pages::editor_file_templates::tests::renders_file_templates`
5. settings pages › edits includes and code templates in their tabs → `settings_view::pages::editor_file_templates::tests::edits_includes_and_code_templates_in_their_tabs`
6. settings pages › keeps a project scheme seeded from Default → `settings_view::pages::editor_file_templates::tests::keeps_a_project_scheme_seeded_from_default` (through the page) and `templates::files_store::tests::keeps_a_project_scheme_seeded_from_default` (the store, with the spec's exact sequence)
7. dialogs › shows the template chooser then the name step → not covered (New File dialog)
8. dialogs › prefills fixed names and follows the kind's extension → not covered
9. dialogs › names an ad-hoc template from another feature → not covered
10. dialogs › chooser filters by typing and picks with Enter → not covered (Chooser popup)
11. dialogs › chooser picks by mnemonic and skips section rows → not covered
12. shows plugin snippets as read-only live templates and withdraws them with the plugin → `settings_view::pages::editor_live_templates::tests::shows_plugin_snippets_as_read_only_live_templates_and_withdraws_them_with_the_plugin` (the snippets are handed to the view by the test; no native contribution source)

`unit:src/features/templates/__tests__/pluginFiles.test.tsx` renders only `NewFileDialogHost`, none of these pages (read whole): not this writer's row.

No `ipc:` row: the three pages call no backend command; they read and write `localStorage` (`src/features/templates/storage.ts:3-18`), which natively is `platform::storage::UiStorage`.

## (c) Behaviours

### Postfix Completion (`src/features/templates/settings/PostfixPage.tsx`)

- `:11` settings read from the store (`openidea.postfix.v1`): built (`stored_postfix_settings`).
- `:13` the first language set (`ts`) shown first: built.
- `:14` the set's templates are those whose id starts with `<set>.`: built (`templates_of`).
- `:19-23` Enable postfix completion, stored at once (`store.ts:22-25` writes all four fields): built.
- `:24-29` Expand templates with Tab, disabled while off: built (drawn at half opacity, click ignored).
- `:30-35` Show postfix templates in code completion, disabled while off: built.
- `:37-41` the hint with `.key`, `user.name.nn`, `items.for` in code: built (the two `<T>` parts through `tx`, the rest as written).
- `:42-55` tabs per language set, the active one selected: built (`settings-postfix-group-*`). The tablist's aria-label "Languages" has no visible counterpart.
- `:56-71` one row per template: checkbox (on unless disabled; disabled while off), `.key` in code, the description with the description as tooltip: built except the tooltip (left out: the description is the row's own text, cut with an ellipsis).
- `sync.ts:7-9` a change stored by another window is shown: built (`observe_global::<UiStorage>`).

### Live Templates (`src/features/templates/settings/LiveTemplatesPage.tsx`)

- `:20-30` `groupTemplates`: built (`group_templates`, same rules).
- `:41-52` Add: id from time and random, group of the selected non-built-in template or User, `abbr`, `$END$`, the selected one's contexts or `any`, selected: built (entropy from `RandomState`, as `run/configs.rs:242` does).
- `:56-67` the two options, stored at once (`store.ts:96-100`): built.
- `:71` Add Template button: built. `:72-82` Remove Template, disabled with no selection or a plugin's template, clears the selection: built (a built-in is hidden, as `store.ts:62-68`).
- `:83-89` Filter field: built (`settings-live-filter`).
- `:91-123` the list: group headers, per template a checkbox (disabled for a plugin's; saves `enabled`), the abbreviation (link colour when a changed built-in, faint when disabled), the description: built. The description's tooltip is left out (it is the row's text).
- `:126-135` the editor, read-only for a plugin's template, or "Select a template to edit it": built (fields read-only, checkboxes ignore clicks, the pane dimmed).
- `:150-161` Abbreviation, stored trimmed: built (the field is rewritten to the trimmed text, as the controlled input shows it).
- `:162-172` Description: built. `:173-183` Group, empty is User: built.
- `:184-199` the template text, a plain textarea (not a code editor): built with `ui::text_area::TextArea` in the code font; Tab inserts a tab at the caret: built (claimed with `input::claim_keys`, inserted with `EntityInputHandler::replace_text_in_range`).
- `:200-204` the variables hint: built.
- `:205-217` Contexts, split at commas and blanks: built. Left out on purpose: the webview's controlled input rewrites the field to `contexts.join(", ")` on every keystroke, which drops a separator as it is typed (typing `rust,` shows `rust`); the native field keeps what is typed and stores the parsed list.
- `:218-224` Also expand in comments and strings (`scope` `any` / `code`): built.
- `:225-276` the variables table (expression, default value, Skip if defined; only the text's variables are kept): built.
- `:277-282` the functions hint: built.
- `:283-289` Restore Default for a changed built-in: built.
- `store.ts:115` / `sync.ts:7-8` contributed snippets and changes stored elsewhere: built (`set_contributed_snippets`, `observe_global::<UiStorage>`; the fields are made again only when the stored text differs from what the page stored itself, so typing never moves the caret).

### File and Code Templates (`src/features/templates/settings/FileTemplatesPage.tsx`, `files/store.ts`)

- `:28-33` file name styles As typed / snake_case / PascalCase / kebab-case: built (`NAME_STYLES`).
- `:35-38`, `:55-65` the Scheme choice, disabled without a project, "Templates of this project only" with Project: built (`set_scheme`; `store.ts:198-205`: a first Project is seeded from Default, the choice stored under `.scheme:<root>`).
- `:40-45`, `:66-79` the Files / Includes / Code tabs: built. `:80-82` a scheme change starts the tab over (`key={scheme}`): built (`start_tab`); switching tabs starts the new one on its first entry (the component remounts): built.
- `:51` the store read when the page opens: built.
- `:87-104` Files: first template selected; Add Template (`user.<time>`, "New Template", `file` icon, `txt`, as typed, one Default kind `$END$\n`): built.
- `:111-121` Remove Template, disabled with no selection, clears the selection (a built-in is hidden, `store.ts:130-135`): built.
- `:123-145` per template a checkbox (saves `enabled`), its icon, its name (link colour when a changed built-in): built (faint when disabled: the CSS `.disabledItem` applies to `.itemLabel` only, which this row does not have; the native row is faint anyway. Hypothesis: the webview shows a disabled file template in the normal colour).
- `:160-165` the kind shown resets when another template is selected (`key={selected.id}`): built.
- `:169-179` Name (aria "Template name"): built. `:180-190` Extension, stored without a leading dot and blanks, placeholder "none": built. `:191-199` File name style: built.
- `:201-240` the kinds as tabs, Add Kind (`kind<time>`, "New kind", `$END$\n`, shown), Remove Kind (disabled under two, the first shown after): built.
- `:241-251` Kind label: built. `:252-258` the kind's text, a plain textarea: built with `TextArea` (no Tab handling here, as the webview has none).
- `:259-263` Reformat according to style (`reformats_on_create`, stored as `reformat`): built.
- `:264-270` the variables hint: built (translated parts through `tx`, the `…);` part as written).
- `:271-277` Restore Default for a changed built-in: built.
- `:313-324` Includes: first include selected, Add Include ("New Include", then "New Include 2"…): built. `:330-341` Remove Include: built.
- `:364-380` the include's name, read-only for a built-in, committed on blur or Enter (empty, unchanged or taken puts the name back): built (blur through `cx.on_blur`, attached at render because it needs the window; Enter commits and is the dialog's OK, as the dialog's Enter handler `src/ui/Dialog.tsx:294-303` also runs).
- `:397-403` the include's text: built. `:404-409` the `#parse("name")` hint: built. `:410-416` Restore Default: built.
- `:421-471` Code: first code template selected, "Used by" and its description, its text, the `#parse` hint, Restore Default when changed: built.
- `store.ts:33-36,99-101` contributed templates treated like built-ins: built in the store (`set_contributed`, `is_base_template`) and the view (`set_contributed_templates`), untested (no native source).
- `store.ts:38-49` an edit laid over its built-in field by field (`{...t, ...custom}`): built (`overlay`); a stored user template that does not read as a `FileTemplate` is left out (the webview would show it with missing fields).
- `sync.ts:16-17` a change stored by another window under any key starting with the Default key: built for the two keys the page shows (the scheme choice and the scheme's customisations); a change to another project's keys does not matter to this page.

## Storage keys each page reads and writes (all in the UI storage, `ui-storage.json` of the configuration folder; never the settings files)

- Postfix Completion: `openidea.postfix.v1` (read and written; `{enabled, expandWithTab, showInCompletion, disabled}`).
- Live Templates: `openidea.liveTemplates.v1` (`{templates: {id: template}, deleted: [ids]}`) and `openidea.liveTemplates.options.v1` (`{expandWithTab, showInCompletion}`), both read and written.
- File and Code Templates: `openidea.fileTemplates.v1` (the Default scheme: `{templates, deleted, includes, deletedIncludes, code}`), `openidea.fileTemplates.v1.scheme:<root>` (`{scheme: "default" | "project"}`) and `openidea.fileTemplates.v1.project:<root>` (the Project scheme, same shape), `<root>` being the project path in its IPC form. The Project key is written once from Default's text the first time Project is chosen.

## (d) Hypotheses

- Measured by reading `settings_view/dialog.rs:163-174` and `platform/input.rs:144,151`: the Settings dialog claims keys only while its search field or its page tree has the focus (exact focus), so Tab, Enter and Escape in the pages' fields are not taken by the dialog, and Submit/Cancel fire once.
- Measured by reading `Cargo.lock`: `serde_json` depends on `indexmap` in this workspace, so `serde_json::Map` keeps insertion order; the store does not rely on it anyway (it parses the stored text straight into `Ordered<T>` and seeds the Project scheme from Default's text as stored).
- Measured by reading `workbench/mod.rs:6653-6669`: the end-to-end fixture `template_project` is the compiled `trusted_project` helper plus `window.activate_window()` (needed for the include name's blur listener).
- Hypothesis: `TextArea::set_soft_wrap(false)` and `set_rows(8, 20)` give a text area about as tall as the webview's `min-height: 140px`. Settle by a look at the real window.
- Hypothesis: closures declared with a `cx: &App` parameter accept a `&mut Context<V>` argument by deref coercion (`mark(…, cx)` in `editor_live_templates.rs`). Settle by compiling; if not, write `&*cx`... or turn the closure into a `fn`.
- Hypothesis: a click on a template's checkbox reaches both the checkbox's and the row's `on_click` (the webview selects and toggles). If GPUI stops at the innermost handler, the row is not selected by that click; nothing asserts it.
- Hypothesis: in the test window and in the 980x720 Settings dialog, the controls clicked by the tests (all near the top of their panes) are inside the visible part; the list is long, so every test that clicks a template filters first.
- Hypothesis: the root the webview's store uses in its keys (`useProject.getState().project.root`) is the backend's IPC form of the path, which the native page builds with `platform::paths::ipc_path(&root.to_string_lossy())` as `workbench/code_insight.rs:386-388` does. Settle by reading `src-tauri`'s `ProjectInfo` root, or by a look at `ui-storage.json` after both apps chose a Project scheme for the same project.
- Hypothesis: `cx.on_blur` on the include name's focus handle fires when a test moves the focus to the include's text in an active window (as `settings_view/pages/build_cmake.rs:321` relies on). Settle by `includes_are_added_renamed_and_removed`.
- Hypothesis: the Input placeholders of the webview ("Filter", "className()", `"value"`, "any, rust, typescript…") are translated; they go through `tx`, which returns them unchanged when the catalog lacks them.

## (e) Tests most likely to fail first

- `includes_are_added_renamed_and_removed`: the blur-commit; and Enter in the include name both committing and emitting Submit.
- The file templates end-to-end tests: the Settings page area is about 560 px high; the controls clicked are in the upper half of the editor pane, but the Reformat box is under an eight-row text area (hypothesis: still visible).

- `edits_a_live_template` and the live end-to-end tests: if `type_over` on a field whose Changed handler calls `set_text` (Abbreviation, Group) re-enters, or if the filter's typing does not reach the field before the click; read the panic's selector.
- `each_control_of_the_editor_is_stored_with_the_template`: `simulate_keystrokes("tab")` reaching the claim depends on `input::init` having installed the keystroke interceptor in the view test (it does through `test_support::init`).
- `enter_and_escape_in_the_fields_answer_the_dialog`: relies on the TextArea emitting `Cancelled` on Escape and inserting a line break on Enter (read in `ui/text_area.rs:804-807`).

## (f) What a look at the real window must check; machine sources

- Postfix: the three options in one wrapped row, the hint with code in the mono font, the six tabs, the bordered list filling the rest of the page.
- Live: the split of 240 px list and the editor; the editor scrolls inside the split when the variables table is long; the text area's height; the modified (link colour) and disabled (faint) abbreviations.
- File and Code Templates: the scheme row, the three tabs, the split with icons per template, the kind tabs, the include name greyed for File Header.
- Machine sources: none. All three pages read only the UI storage (`ui-storage.json` in the configuration folder). No secret, no SSH, Docker, WSL, network, toolchain or keychain is read by these pages or their tests.

## (g) What these pages, the SSH page and the Deployment page should share

- The template pages and the previous writer's TODO/Log/GitHub pages each mount a whole-page view and answer Submit/Cancel the same way (`workbench/settings_template_pages.rs::mount`, `workbench/settings_feature_pages.rs::mount_page`): once both compile, one `mount_page_view` helper (the `Slots::page_view` of `custom_controls.rs` does the same for its pages) should serve all.
- The 240 px list-plus-editor split (template pages, SSH configurations, Deployment servers) should become one shared `split_editor` layout helper in `pages/mod.rs`, with the toolbar of add/remove icon buttons.
- The labelled text row (`labelled` here, the forms of `tools_ssh/form.rs` and `build_deployment`) should share one helper.
