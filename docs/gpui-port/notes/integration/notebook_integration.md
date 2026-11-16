# Notebook editor (`.ipynb`): integration notes

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Nothing here was compiled or run. Every new file is untracked; no existing file was edited.

## New files

| File                                                            | Lines | What                                                                                                                                                                                                                                                                                                                   |
| --------------------------------------------------------------- | ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/notebook/mod.rs`                                  | 62    | `NOTEBOOK_KIND`, `is_notebook_path`, `notebook_input`, `notebook_title`, `notebook_icon`; declares the two test files                                                                                                                                                                                                  |
| `app-gpui/src/notebook/model.rs`                                | 295   | `NotebookModel` (entity): wraps `features_a::notebook::session::NotebookSession`, emits `NotebookModelEvent::Send(Vec<Request>)`, `invoke` / `wait` / `discard_and_resume`, interpreter persistence (`UiStorage` key `openidea.notebook.python:<path>`), environments list, selected cell, Markdown cells being edited |
| `app-gpui/src/notebook/view.rs`                                 | 700   | `NotebookView`: toolbar, error bar, key claims (⌘S, ⌘Z, ⇧⌘Z, ⌘Y, ⇧Enter / Ctrl+Enter), one `ui::TextArea` per shown source, reload prompt, menus as `NotebookViewEvent::ShowMenu`, links as `NotebookViewEvent::OpenLink`                                                                                              |
| `app-gpui/src/notebook/cells.rs`                                | 775   | the cells, cell toolbars, Markdown preview drawing, outputs drawing; accessors `output_texts`, `preview_text`                                                                                                                                                                                                          |
| `app-gpui/src/notebook/output_items.rs`                         | 323   | outputs → `OutputItem` (text with ANSI runs and 200 000-char truncation, PNG/JPEG from base64, HTML as text, inert JS, SVG not drawn, notices); `html_text`, `decode_entities`                                                                                                                                         |
| `app-gpui/src/notebook/ansi.rs`                                 | 201   | SGR parser (16 colours, 256 palette, 24-bit, bold, italic, underline); other escapes dropped                                                                                                                                                                                                                           |
| `app-gpui/src/notebook/markdown_cell.rs`                        | 249   | Markdown cell blocks from `features_b::viewers::render::render_markdown` with images resolved (attachment → data URL, data PNG/JPEG kept, all else → alt text); `MarkdownCache` (rebuild only on source or referenced-attachment change)                                                                               |
| `app-gpui/src/notebook/tests/proofs24_notebook_units.rs`        | 257   | 7 OutputView cases + 4 security cases                                                                                                                                                                                                                                                                                  |
| `app-gpui/src/notebook/tests/proofs24_notebook_view.rs`         | 314   | 10 GPUI view tests (2 of them OutputView cases)                                                                                                                                                                                                                                                                        |
| `app-gpui/src/workbench/notebook.rs`                            | ~535  | tab kind builder, per-window `OpenNotebooks` global, every backend call, output channel bridge, `NotebookKernel` listener, close guard, `save_notebooks`, `document_history`, `Workbench::notebook_view`, `Workbench::open_notebook`                                                                                   |
| `app-gpui/src/workbench/tests/proofs24_e2e_notebook.rs`         | 334   | 9 end-to-end tests against the real backend                                                                                                                                                                                                                                                                            |
| `app-gpui/src/workbench/tests/proofs24_e2e_notebook_pending.rs` | 47    | NOT declared: Run All with outputs, waits for the kernel seam below                                                                                                                                                                                                                                                    |

## (a) Hunks for existing files, in order

### 1. `app-gpui/src/lib.rs`: the module

Anchor (exact):

```
#[cfg(debug_assertions)]
pub mod inspect;
pub mod platform;
```

New:

```
#[cfg(debug_assertions)]
pub mod inspect;
pub mod notebook;
pub mod platform;
```

### 2. `app-gpui/src/workbench/mod.rs`: the child module

Anchor (exact):

```
pub mod menu_spec;
pub mod notification_balloons;
```

New:

```
pub mod menu_spec;
mod notebook;
pub mod notification_balloons;
```

### 3. `app-gpui/src/workbench/mod.rs`: the end-to-end tests (inside `mod tests`)

Anchor (exact):

```
    mod proofs23_e2e_terminal_groups;
    mod proofs3_context;
```

New:

```
    mod proofs23_e2e_terminal_groups;
    mod proofs24_e2e_notebook;
    mod proofs3_context;
```

Do not declare `proofs24_e2e_notebook_pending`.

### 4. `app-gpui/src/workbench/tab_views.rs`: the tab kind (end of `TAB_KINDS`)

Anchor (exact):

```
        build: super::vcs_text_actions::build_external_log_tab,
    },
];
```

New:

```
        build: super::vcs_text_actions::build_external_log_tab,
    },
    TabKind {
        id: crate::notebook::NOTEBOOK_KIND,
        restorable: true,
        active_key: None,
        focus_key: None,
        title: crate::notebook::notebook_title,
        icon: crate::notebook::notebook_icon,
        build: super::notebook::build_notebook_tab,
    },
];
```

### 5. `app-gpui/src/workbench/tab_views.rs`: `.ipynb` opens as a notebook (`open_project_file`)

Anchor (exact):

```
    pub fn open_project_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tabs) = self.tabs() else { return };
```

New:

```
    pub fn open_project_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if crate::notebook::is_notebook_path(&path.to_string_lossy()) && self.tabs().is_some() {
            return self.open_notebook(path, window, cx);
        }
        let Some(tabs) = self.tabs() else { return };
```

`open_project_file_at` calls `open_project_file`, so links and Search Everywhere results reach it too. Hypothesis: Open File… and Search Everywhere open project files through `open_project_file`; settle with `grep -rn "open_project_file\b" app-gpui/src/workbench app-gpui/src/everywhere`.

### 6. `app-gpui/src/workbench/mod.rs`: Save All saves notebooks (`fn save_all`)

Anchor (exact, the whole function):

```
    /// Saves every unsaved editor; the first failure's message, if any.
    fn save_all(&self, cx: &App) -> impl Future<Output = Result<(), String>> + 'static + use<> {
        let saving: Vec<_> = self
            .editors()
            .iter()
            .filter(|editor| editor.read(cx).is_dirty())
            .map(|editor| editor.read(cx).save(cx))
            .collect();
        async move {
            let mut failure = None;
            for save in saving {
                match save.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => failure = failure.or(Some(error.message)),
                    Err(error) => failure = failure.or(Some(error.to_string())),
                }
            }
            failure.map_or(Ok(()), Err)
        }
    }
```

New (signature unchanged, so the six callers stay as they are; the notebooks are saved when the future runs, outside any entity update):

```
    /// Saves every unsaved editor and notebook; the first failure's message, if any.
    fn save_all(&self, cx: &App) -> impl Future<Output = Result<(), String>> + 'static + use<> {
        let saving: Vec<_> = self
            .editors()
            .iter()
            .filter(|editor| editor.read(cx).is_dirty())
            .map(|editor| editor.read(cx).save(cx))
            .collect();
        let app = cx.to_async();
        let label = self.label.clone();
        async move {
            let mut failure = None;
            for save in saving {
                match save.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => failure = failure.or(Some(error.message)),
                    Err(error) => failure = failure.or(Some(error.to_string())),
                }
            }
            for save in app.update(|cx| notebook::save_notebooks(&label, cx)) {
                match save.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => failure = failure.or(Some(error.message)),
                    Err(error) => failure = failure.or(Some(error.to_string())),
                }
            }
            failure.map_or(Ok(()), Err)
        }
    }
```

### 7. `app-gpui/src/workbench/edit_actions.rs`: Edit › Undo / Redo in a notebook (`fn run`)

Anchor (exact):

```
    // Documents keep no history apart from their editor's: the editor's chain answers for them.
    match route_edit_command(command, edit_focus(window, cx), |_| false) {
```

New:

```
    // A notebook keeps its history in the backend: Undo and Redo while it has the keyboard go there.
    if super::notebook::document_history(command, window, cx) {
        return Ok(());
    }
    // Other documents keep no history apart from their editor's: the editor's chain answers for them.
    match route_edit_command(command, edit_focus(window, cx), |_| false) {
```

Without this hunk the cell's `TextArea` (a registered `EditTarget`) answers the menu's Undo with its own history, which the notebook must never use. ⌘Z / ⇧⌘Z / ⌘Y typed in a cell are claimed by the view and do not need it.

No action is registered: the webview's notebook feature registers none (`src/features/notebook/index.ts` has only the editor kind and the file editor provider). No menu entry.

## (b) Parity rows

Test names are relative to `openidea_gpui::`. Prefixes: `U` = `notebook::tests::proofs24_notebook_units::`, `V` = `notebook::tests::proofs24_notebook_view::`, `E` = `workbench::tests::proofs24_e2e_notebook::`.

| Key                                              | Expected                  | Proof                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | Missing for done                                                                                                                                                                                                                                                                                                                             |
| ------------------------------------------------ | ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `point.registerEditorKind:notebook`              | partial                   | `E::a_notebook_file_opens_in_a_notebook_tab_and_saves_the_typed_source` (kind, title `a.ipynb`, no text editor), `workbench::notebook::tests::the_notebook_kind_is_restorable_and_titled_by_its_file`, `E::undo_and_redo_of_the_toolbar_go_through_the_notebooks_history`, `E::reload_from_disk_asks_then_drops_unsaved_changes_for_the_files_content`, `E::closing_the_last_tab_of_a_notebook_releases_its_backend_document`, `E::an_unsaved_notebook_asks_before_its_tab_closes`                                                                                                                                                                                     | session restore not proven by a test (restorable flag only); no dirty mark is drawn on any tab of the app (`proofs18_e2e_editor_pending.rs` says so); Save All and Edit-menu Undo are hunks 6 and 7 with no test of their own; no automatic reload when the file changes on disk (the webview has none either: reload is the toolbar button) |
| `point.registerFileEditorProvider:notebook`      | done                      | `E::a_notebook_file_opens_in_a_notebook_tab_and_saves_the_typed_source`, `notebook::tests::notebook_files_open_as_notebook_tabs_titled_by_their_name`; the tab key is `notebook:<canonical path>` from `notebook_open`                                                                                                                                                                                                                                                                                                                                                                                                                                                 | — (an unopenable file still gets the notebook tab, as the webview; not tested)                                                                                                                                                                                                                                                               |
| `feature:notebook`                               | partial                   | all of `V::*` and `E::*`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | see (c): no syntax colours or line numbers in cell editors, no virtualised list, no scroll position kept, no spinner, HTML shown as text, SVG not drawn, interpreter menu without the path detail, no `<details>` toggling in Markdown previews, outputs of a successful run not proven end to end                                           |
| `ipc:notebook_open`                              | done                      | `E::a_notebook_file_opens_in_a_notebook_tab_and_saves_the_typed_source` (canonical key and cells from the file)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | —                                                                                                                                                                                                                                                                                                                                            |
| `ipc:notebook_get`                               | done                      | `E::typing_after_a_change_behind_the_editor_is_rebased_on_the_current_notebook` (the typed edit lands next to a change only `get` returns)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | —                                                                                                                                                                                                                                                                                                                                            |
| `ipc:notebook_edit`                              | done                      | `E::a_notebook_file_opens_in_a_notebook_tab_and_saves_the_typed_source`, `E::typing_after_a_change_behind_the_editor_is_rebased_on_the_current_notebook` (backend document holds the typed source)                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | —                                                                                                                                                                                                                                                                                                                                            |
| `ipc:notebook_save`                              | done                      | `E::a_notebook_file_opens_in_a_notebook_tab_and_saves_the_typed_source` (file on disk holds the typed source)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | —                                                                                                                                                                                                                                                                                                                                            |
| `ipc:notebook_undo`                              | done                      | `E::undo_and_redo_of_the_toolbar_go_through_the_notebooks_history` (backend document back to the original)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | —                                                                                                                                                                                                                                                                                                                                            |
| `ipc:notebook_redo`                              | done                      | same test                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | —                                                                                                                                                                                                                                                                                                                                            |
| `ipc:notebook_reload`                            | done                      | `E::reload_from_disk_asks_then_drops_unsaved_changes_for_the_files_content` (cell shows the file's new content)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | —                                                                                                                                                                                                                                                                                                                                            |
| `ipc:notebook_close`                             | done                      | `E::closing_the_last_tab_of_a_notebook_releases_its_backend_document` (`project.notebooks.get(id)` fails afterwards)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | —                                                                                                                                                                                                                                                                                                                                            |
| `ipc:notebook_execute`                           | partial                   | `E::a_kernel_that_cannot_start_shows_why_and_restart_recovers` (the stand-in interpreter is started and its death reported), `E::a_notebook_in_restricted_mode_starts_no_kernel`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | outputs and execution counts of a successful run (needs the kernel seam; `proofs24_e2e_notebook_pending.rs`)                                                                                                                                                                                                                                 |
| `ipc:notebook_interrupt`                         | done                      | `E::interrupt_kernel_stops_a_batch_whose_kernel_is_starting` (the started process is gone after Interrupt)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | (interrupt of a running cell needs the seam; the call and its effect are proven)                                                                                                                                                                                                                                                             |
| `ipc:notebook_restart`                           | partial                   | `E::a_kernel_that_cannot_start_shows_why_and_restart_recovers` (kernel failure recovered, state idle), `E::a_notebook_in_restricted_mode_starts_no_kernel` (refused with the backend's reason)                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | restart of a live kernel (seam)                                                                                                                                                                                                                                                                                                              |
| `event:NotebookKernel`                           | done                      | `E::a_kernel_that_cannot_start_shows_why_and_restart_recovers` (state `dead` from the backend's event), `V::a_kernel_failure_naming_ipykernel_shows_the_install_hint`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | —                                                                                                                                                                                                                                                                                                                                            |
| `unit:src/features/notebook/OutputView.test.tsx` | partial, 9 of 9 by name   | `U::leaves_native_interactions_such_as_details_alone_and_only_claims_links`, `V::opens_web_links_with_the_system_handler`, `U::keeps_the_rendered_preview_across_snapshots_with_equal_attachments`, `U::resolves_an_entity_encoded_attachment_in_a_markdown_destination_a_reference_definition_and_raw_html`, `U::resolves_an_attachment_named_proto_or_constructor_from_parsed_notebook_json`, `U::does_not_resolve_inherited_members_as_attachments`, `U::does_not_rebuild_when_only_an_unreferenced_attachment_changes`, `U::ignores_attachment_identity_when_the_source_references_none`, `V::does_not_re_sanitize_an_unchanged_svg_when_a_fresh_snapshot_arrives` | two check a neighbour of the case: "opens web links" asserts the link reaches the window and resolves to an external URL (the window's `cx.open_url` is not observed); "re-sanitize SVG" checks that a PNG is not decoded again (SVG is not drawn natively)                                                                                  |
| `unit:src/features/notebook/security.test.ts`    | partial, 4 of 4 by name   | `U::keeps_raw_markdown_markup_from_reaching_the_page` (all 8 `it.each` inputs), `U::retains_table_content_while_removing_executable_navigation_and_external_resource_surfaces`, `U::retains_safe_svg_chart_paint_local_definitions_and_raster_images_but_removes_active_external_svg`, `U::resolves_only_embedded_image_attachments_and_makes_markdown_file_links_editor_controlled`                                                                                                                                                                                                                                                                                   | the SVG case asserts that SVG is never drawn (its plain text shows) instead of a sanitised SVG keeping paint                                                                                                                                                                                                                                 |
| `unit:src/features/notebook/outputs.test.ts`     | done (8 of 8, existing)   | `features_a::notebook::outputs::tests::*` (8, names unchanged); the code is now reached through `NotebookModel::output_events`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | —                                                                                                                                                                                                                                                                                                                                            |
| `unit:src/features/notebook/session.test.ts`     | done (16 of 16, existing) | `features_a::notebook::session::tests::*` (16, names unchanged); the code is now reached through `NotebookModel`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | —                                                                                                                                                                                                                                                                                                                                            |

Other tests (not rows on their own): `notebook::ansi::tests::*` (3), `notebook::output_items::tests::*` (5), `notebook::markdown_cell::tests::data_raster_images_stay_and_other_images_show_their_text`, `notebook::cells::tests::data_urls_name_their_raster_format`, `V::the_toolbar_follows_the_notebooks_state`, `V::shift_enter_runs_the_selected_code_cell_and_breaks_the_line_elsewhere`, `V::typing_queues_a_source_edit_and_cmd_z_undoes_in_the_notebook_history`, `V::the_cell_toolbar_moves_inserts_deletes_and_retypes_cells`, `V::markdown_cells_switch_between_preview_and_source`, `V::raw_and_unknown_cells_are_shown_read_only`, `V::reload_from_disk_asks_before_it_discards`.

## (c) Behaviours of the webview's notebook editor

`src/features/notebook/index.ts`

- :13-16 provider acquisitions (lease released if the open is rolled back): left out, the native tab opens after the backend open and nothing is rolled back; a second open of an open notebook returns the same backend id.
- :19-45 document lifecycle: isDirty → `TabView.dirty`; save → Save All hunk 6 and the toolbar/⌘S; prepareClose / cancelClose / discard → the close guard (`close_decision`); close → `tab_closed` (last tab of the window); undo / redo → hunk 7 and the view's keys. Built. flush on its own is used by Save; autosave of notebooks: left out (the native autosave goes through `fn autosave`, text editors only).
- :49-56 editor kind, restorable, title = file name, icon `file`: built (hunk 4).
- :57-85 provider for `/\.ipynb$/i`, canonical path from `notebook_open`, key `notebook:<path>`, failure still opens the notebook surface: built (`open_notebook`, hunk 5). The "notebook changed while opening the editor" error (:74-75): left out (the model keeps its snapshot; Hypothesis: cannot happen because the backend returns the same incarnation id while it is open).

`src/features/notebook/NotebookEditor.tsx`

- :23 empty state "Notebook path is missing": left out (a notebook input always has a path; the builder refuses one without).
- :31,41-44 selected cell kept in the session: built (`NotebookModel::selected`).
- :34 active cell = selected else first code cell: built (`active_cell`).
- :35-40 failures of UI operations reported in the error bar: built (`NotebookModel::invoke`).
- :45-52 virtualised cell list with remembered scroll offset: left out (all cells drawn in one scrolling element; scroll offset not kept across tabs). Large notebooks draw slower.
- :53-55 load on mount: built (`ensure_loaded` after the label is known).
- :56-67 browser `historyUndo` / `historyRedo` input events: built as hunk 7 (Edit menu).
- :69-76 Markdown at the preview font size, code and outputs at the editor font size and line height: partly; code and outputs use the theme's mono family at the UI size; the settings are not read (left out: no setting lookup wired).
- :77-78 disabled / history-disabled rules: built (`toolbar`).
- :79-85 interpreter options with the current one prepended when not detected: built (`show_python_menu`); the option's path detail is not shown (menu rows have one label).
- :92-112 ⌘/Ctrl+S save, ⌘/Ctrl+Z undo, ⇧⌘Z and ⌘Y redo, Shift/Ctrl+Enter run active code cell; IME composition passes: built (`key_command`, claims on the root and on every cell editor). Composition: Hypothesis: the claim sees no keystroke while an input method composes; settle by typing with an IME in the real window.
- :114-184 toolbar Run Cell (primary), Run All, Interrupt Kernel, Restart Kernel, separator, Python interpreter select, Refresh Python interpreters, kernel state (busy while running, error as its tooltip, spinner), spacer, Undo, Redo, Save, Reload Notebook from Disk: built; the spinner is a static `refresh` icon (no spinner component); the Save icon name `save` is not in the native icon table (falls back to the file icon).
- :185-216 error bar: message, ipykernel install hint, "Uncommitted source edits" with each pending source, Retry Pending Edits (flush), Retry (load): built; the pending sources are always shown (no `<details>` collapse).
- :217-224 Loading notebook… / Notebook unavailable + "Notebook editing and execution require the desktop app.": built (text kept as the catalog has it).
- :233-245 "This notebook has no cells" with Add Code Cell (disabled only when frozen): built.
- :273-292 add bar: Add Code Cell, Add Markdown Cell after the last cell, disabled while running: built.
- :295-313 reload dialog (Cancel / Discard Changes and Reload, warning) then discard and resume: built (`ask_reload`, native prompt).
- :337 Markdown editing state kept in the session: built (`markdown_editing`).
- :341-345 cell section selected border, select on click and on focus, aria-label "Cell N: type": built except the aria-label (no accessibility tree).
- :348-350 execution count `[n]` / `[ ]` for code, index+1 otherwise: built.
- :351-365 cell type select (Code, Markdown, Raw, the unknown type itself), disabled while frozen/running or for unknown non-raw cells: built (menu through the workbench).
- :366-376 per-cell Run Cell: built.
- :377-389 Edit / Preview Markdown toggle: built.
- :391-418 Move Cell Up / Down (not past the ends), Insert Code Cell Below, Delete Cell, disabled while frozen or running: built.
- :420-424 raw / unknown cell notice and read-only source: built.
- :425-448 Markdown preview, double click to edit, empty cell shows Edit Markdown: built.
- :450 code and Markdown sources in an editor: built with `ui::TextArea` (see CellEditor below).
- :452 outputs of code cells: built.

`src/features/notebook/CellEditor.tsx`

- :43-53 theme, syntax highlighting, line numbers, selection drawing, active line, special characters, bracket matching, indent on input: left out. The app's `Editor` needs a backend text document (`Editor::with_text` takes a `DocumentInfo` and edits through `Documents::edit`; `Documents::open_virtual` is read-only), so a cell is edited in `ui::TextArea` (the app's multi-line input). Proposed seam (not written): `Documents::open_memory(uri, language, text) -> DocumentInfo`, an editable document never written to disk, so a cell could use `Editor` with Python / Markdown colours.
- :54-78 Mod-z / Mod-Shift-z / Mod-y to the notebook history: built (claims).
- :79 indentWithTab and the default keymap: partly (TextArea's own keys; Tab is not claimed, Hypothesis: Tab inserts a tab character in TextArea, settle in the real window).
- :81,87-89,128 read-only while frozen, external changes allowed: built (`set_read_only`, `set_text` without `Changed`).
- :82-83 line wrapping, max height 640 px then scroll: built approximately (TextArea soft-wraps; 2 to 32 rows).
- :84-86 aria-label "Python cell source" / "Markdown cell source": left out (no accessibility tree).
- :90-95 every typed change becomes `setSource`: built (`TextAreaEvent::Changed`).
- :97-101 editor state kept across remounts: built for as long as the view lives (the `TextArea` entity per cell); a second tab of the same file has its own editors.
- :105-112 language support loaded per cell type: left out (see :43-53).
- :123-127 a source changed outside replaces the text: built; the caret goes to the end (TextArea `set_text`), the webview keeps the selection mapping. Hypothesis: a source with `\r\n` would be replaced on every model change because `TextArea` normalises line breaks; settle with a CRLF notebook.
- :133 no local history: built (claims; hunk 7).

`src/features/notebook/OutputView.tsx` and `security.ts`, what each output type does:

- stream / error (`AnsiText`, 200 000 chars then notice): webview draws ANSI-coloured text; native the same (`ansi.rs`, `PREVIEW_LIMIT`).
- `image/png`, `image/jpeg`: webview `<img>` from a `data:` URL built from the bundle's base64; native decodes the same base64 bytes from the file (`GpuiImage::from_bytes`), no size limit (the webview has none).
- `image/svg+xml`: webview sanitises with DOMPurify and an allowlist, then `<img>`; native does not draw it and shows the bundle's `text/plain` (or "Notebook image output"). Reason: GPUI's SVG decoder follows `<image href>` references (Hypothesis, not read in `usvg`), and no sanitiser exists natively.
- `text/html`: webview renders sanitised static HTML in a sandboxed iframe with a CSP; native shows "Static notebook HTML output" and the HTML's text (script, style, iframe, object, embed, noscript, template, svg, math, head, title, textarea dropped with their content; every tag and attribute dropped).
- `application/javascript`, `text/javascript`: webview notice + escaped source in a sandboxed iframe; native notice + the source as text. Never run in either.
- other MIME types: notice + the types; unknown output types: notice + the type. Same in both.
- Markdown cells: webview `marked` + DOMPurify, links made inert and opened by the editor (files in the editor, web pages with `openExternalUrl`), only `attachment:` and `data:` PNG/JPEG images; native `render_markdown` (drops style, script, iframe and the like; keeps no style attribute), links opened by the window (`resolve_link`: file → editor at the line, web → `cx.open_url`, anchors inert), images as in the webview, every other image shows its alternative text. Nothing named in a notebook is fetched by either.
- `<details>` in Markdown: webview native toggling; native shows summary and body always (left out: no toggle state).

Restricted project: the backend refuses `notebook_execute`, `notebook_restart` and the SDK detection without trust (`host.require_trust`); open, edit, save, undo, redo, reload, interrupt and close work. The native window makes the same calls, so it behaves the same; proven by `E::a_notebook_in_restricted_mode_starts_no_kernel` (the stand-in's marker never appears).

## Corrections to the existing native models (`app-gpui/src/features_a/notebook/`)

- `session.rs`: `NotebookViewState` has no `environments`, and `Answer::Python` carries only the effective interpreter, so the detected list was dropped. Worked around in `NotebookModel::set_environments` (the driver sets the list before answering).
- `session.rs`: `select_python` does not persist the choice (the webview writes `localStorage["openidea.notebook.python:" + path]`). Done in `NotebookModel::select_python` with `UiStorage`.
- `session.rs`: no public way to call `OutputReducer::recover`, so the webview's bound of 256 buffered output events per frame with recovery (`session.ts:352-357`) cannot be built; the driver applies every event (unbounded queue). Proposed addition: `pub fn output_overflow(&mut self)` calling `self.output.recover()` and `start_refresh()`.
- `session.rs`: `history()` fails with "Notebook is busy or closing." when no snapshot is loaded; the webview loads first (`session.ts:303`). Not worked around (the toolbar disables Undo without a snapshot).
- `session.rs`: `selectedCell`, `scrollTop`, `markdownEditing` of the webview session are absent; the first and last are in `NotebookModel`, the scroll offset is left out.
- `view.rs`: `OutputImage::Svg` says "to sanitize before drawing", but no sanitiser exists; the native view never draws it.

## Proposed seam in `crates/ide-notebook` (not written)

The kernel is `<python> -m ipykernel_launcher -f <connection.json>` speaking ZeroMQ with HMAC-signed messages on five loopback ports (`kernel/process.rs:29-99`, `kernel/transport.rs`); a `/bin/sh` script cannot answer it, so no test can make a run succeed. Smallest seam: a `KernelLauncher` trait in `kernel.rs` (`fn execute(&self, notebook, python, code, cancel, on_output) -> BoxFuture<IdeResult<ExecutionResult>>` plus interrupt / restart / shutdown), `KernelManager::with_launcher(runtime_dir, on_status, Arc<dyn KernelLauncher>)` with today's process launcher as the default, and `NotebookService::with_kernels` in `ide-host` that the project constructor can take in tests. Then `proofs24_e2e_notebook_pending.rs` becomes declarable (it also needs a `crate::notebook::test_support::install_kernel_launcher`).

## (d) Hypotheses (assumed without compiling) and how to settle them

1. `AsyncApp::update` returns the closure's value directly and `Entity::update` with `AsyncApp` / `AsyncWindowContext` returns `R` (read in `gpui-pre-0.3.12/src/app/async_context.rs:176,443`); Hypothesis: `gpui_kit` re-exports exactly these. Settle: compile.
2. `futures::channel::mpsc::UnboundedSender<NotebookOutputEvent>` is `Send + Sync`, as `ide_host::ops::Channel::new` requires. Settle: compile.
3. `Window::prompt` accepts `&[&str]` (as `workbench/mod.rs:1718` does) and `PromptLevel` is exported by `gpui_kit`.
4. `gpui_kit` exports `Rgba` with public `r, g, b, a` fields, `UnderlineStyle`, `InteractiveText`, `ImageFormat::{Png, Jpeg}`, `WindowId`, `Task`, `AsyncApp`.
5. `Styled::font(Font)` exists on `Div` (read in `gpui-pre-0.3.12/src/styled.rs:720`).
6. `ClickEvent::click_count()` and `capture_any_mouse_down(cx.listener(…))` compile on a `Stateful<Div>` (used that way in `project_tree.rs:821`, `workbench/mod.rs:4258` without `cx.listener`).
7. `IdeResult::Ok(…)` through the type alias compiles (it does for enum aliases).
8. `render_markdown` decodes no HTML entities in image sources (read: `image_of` stores the raw attribute); `decode_entities` handles them. Reference-style images (`![a][r]` + `[r]: …`): Hypothesis: `render.rs` does not resolve them, so that third input of `U::resolves_an_entity_encoded_attachment_…` fails. Settle: run the test; the fix is in `render.rs` (another agent's file) or a pre-pass in `markdown_cell.rs`.
9. The `<details>` and `<summary>` tags survive `render_markdown` as `Node::Element` (`KEPT` lists both).
10. A test window redraws during `run_until_parked`, so image decoding (done while drawing) is counted in `V::does_not_re_sanitize_…`.
11. Binding the kernel's five loopback ports in `E::a_kernel_that_cannot_start_…` and `E::interrupt_kernel_…` is not "the network" in the sense of the test rules (127.0.0.1 only, nothing leaves the machine). The integrator decides; if it is, both tests move to the pending file.
12. `SdkDetection` (a test global already read by production code in `settings_view/pages/languages_sdk.rs:239`) is the sanctioned way to keep notebook tests from detecting the machine's Pythons; it guards only when a test sets it, as before.
13. The stand-in interpreter's death is noticed within the 250 ms startup probe and the error text contains "kernel exited" and "ipykernel" (read in `kernel/process.rs:153-160` and `kernel/transport.rs:493-498`).
14. `KernelManager::execute` cancels startup on interrupt and shuts the process down (read in `kernel.rs:166-177`), so the waiting stand-in is gone after Interrupt.
15. `app.unsaved.save`, `app.unsaved.discard`, `common.cancel` read "Save", "Don't Save", "Cancel" in English (the existing close test answers "Don't Save").
16. `cmd-s` and `cmd-z` reach the view's key claims before the keymap (claims run first while the claimed focus is focused, `platform/input.rs:116`).

## (e) Tests most likely to fail first

- `U::resolves_an_entity_encoded_attachment_…` (reference definition input; hypothesis 8).
- `V::does_not_re_sanitize_…` if images are not decoded until a real frame paints (hypothesis 10).
- `V::the_cell_toolbar_moves_inserts_deletes_and_retypes_cells`: counts on the queue keeping the delete behind the unanswered insert (pending = 2) and on simulated clicks reaching the 20 px buttons.
- `V::opens_web_links_with_the_system_handler`: the click must land on the link run of the `InteractiveText` (selector `notebook-cell-0-text-0` is the run's wrapper).
- `E::a_notebook_file_opens_…` and every `E::*` that types: `simulate_input` must reach the focused `TextArea` after `select_all`; the `wait_long` on `state.python.is_some()` needs the `SdkDetection` answer.
- `E::an_unsaved_notebook_asks_before_its_tab_closes`: depends on `release_closed_views` calling the close guard for a dirty view tab and restoring it after Cancel (code read, never exercised by another kind).
- `E::interrupt_kernel_stops_a_batch_whose_kernel_is_starting`: timing of the marker versus the click; `kill -0` on a reused pid is unlikely within the test.
- Clippy: `#[allow(clippy::too_many_arguments)]` is used twice as the backend does; the reviewer may prefer `#[expect]`.

## (f) What a look at the real window must check

- A `.ipynb` from the Project view, Open File and Search Everywhere opens one notebook tab titled by the file name; a second open brings it to the front; a split shows the same cells and edits in one appear in the other.
- The toolbar wraps at narrow widths, the interpreter field shows the interpreter or "Select Python interpreter", the kernel state reads idle / busy / dead with the error as tooltip.
- Cell borders, the accent border on the selected cell, the cell toolbar row, the outputs area below code cells, the 640 px output limit.
- Typing in a cell, ⇧Enter runs, ⌘Z / ⇧⌘Z undo and redo across cells (backend history), the caret after an undo (goes to the end of the text).
- Markdown preview: headings, lists, code blocks, tables, links (a file link opens the file at its line, a web link the browser), embedded attachment images, remote images showing only their alt text.
- Outputs: ANSI-coloured traceback, PNG output at its size, the truncation notice, the inert JavaScript notice, HTML as text.
- The close question on an unsaved notebook, and Save All saving notebooks.
- Dark theme colours of the error bar (`danger`) and of the cell toolbar (`bg-elev`).
