# Untitled documents, Save As, the byte order mark, files changed on disk: integration notes

Draft files, not declared, not compiled. Every anchor below is copied from `git show HEAD:<file>` at HEAD `cae23afb0` (branch `gpui`). A statement not read from the code is marked Hypothesis. Nothing here was compiled or run.

## (a) The list

| Row (parity key)                                                                                                                                                                                                                                              | What the webview does (HEAD `file:line`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | What is native at HEAD                                                                                                                                                                                         | Verdict                                                                                                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:file.newUntitled`                                                                                                                                                                                                                                     | `src/features/app/windows.ts:496-509`: title `app.action.newFile` ("New Untitled File"), category File, icon `plus`; `desktopOnly()` (only refuses outside Tauri, `windows.ts:39-43`); a window with neither project nor LightEdit first calls `openLightEditUntitled()`; then `editorService().openUntitled({focus: true})` → `EditorController.openUntitled` (`src/editor/core/controller.ts:1210-1218`): `backend.openUntitled(null)`, `manager.adopt`, a tab with `untitledInput(uri)` = `{kind: "untitled", key: uri, path: uri}` (`controller.ts:144`). Offered in File › New › Untitled File (`src-tauri/src/menu.rs:229`)                                                                                                                                                                                                                                                                                   | `git grep -n "file.newUntitled" HEAD -- app-gpui/src`: nothing; only `assets/gen/actions.json:3728` and the keymaps                                                                                            | build                                                                                                                                                                                                                                             |
| `ipc:doc_open_untitled`                                                                                                                                                                                                                                       | `src/editor/core/backend.ts:86` (`openUntitled`); call sites `controller.ts:1214` (mint, `null`) and `src/editor/core/model/document.ts:833` (re-acquire a known `untitled:` key: a second tab / split shares the buffer). Adapter `src-tauri/src/commands/doc.rs:33-39` → `ide_host::ops::doc::open_untitled` (`crates/ide-host/src/ops/doc.rs:27-30`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | no `open_untitled` under `app-gpui/src`                                                                                                                                                                        | build                                                                                                                                                                                                                                             |
| `ipc:doc_save_as`                                                                                                                                                                                                                                             | `backend.ts:87`; one call site `controller.ts:1234` (`saveUntitled`): save dialog titled "Save As", `defaultPath` = `<project root>/Untitled-N`, or in LightEdit the folder of the most recently used file tab, or the bare name (`saveAsSuggestion`, `controller.ts:1253-1269`); filters Text files first, languages, All files last (`controller.ts:172-194`); on a path: `backend.saveAs`, `manager.retarget`, every tab with the old key becomes `fileInput(path)` (`controller.ts:1272-1288`); on refusal an error notification "Cannot save file" with the message (`controller.ts:1239-1246`). Reached from an explicit save of a dirty untitled buffer (`document.ts:675-680`: `auto` never writes it) and from the close question (`controller.ts:1611-1640`). There is NO Save As for a file tab: `grep -rn "saveAs\|Save As" src` finds only this flow, and `src-tauri/src/menu.rs` has no Save As entry | only `save_as` of window layouts (`workbench/app_actions/tests/proofs20_layouts.rs:15`)                                                                                                                        | build                                                                                                                                                                                                                                             |
| `unit:src/editor/core/untitled.test.ts` (7 cases)                                                                                                                                                                                                             | "opens editable, pathless buffers with incrementing names"; "autosave never writes an untitled buffer; explicit save asks for a path and retargets tab and model"; "offers the registered languages as save-as types"; "keeps the buffer open and dirty when the save dialog is cancelled"; "asks about a dirty untitled buffer when its last tab closes"; "closes an untitled buffer without saving when the user declines"; "reuses the buffer for a re-opened untitled name and shares it across tabs"                                                                                                                                                                                                                                                                                                                                                                                                           | nothing                                                                                                                                                                                                        | build (6 of 7; the filter half of case 3 is `cannot be tested`: see (d))                                                                                                                                                                          |
| `keymap:vscode-macos`, `module:src/platform/keymaps/vscode-macos.ts`                                                                                                                                                                                          | `src/platform/keymaps/vscode-macos.ts:198` binds `Meta+N` to `file.newUntitled`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | the binding is loaded (`assets/gen/keymaps.json:5464`) but no handler: ⌘N does nothing                                                                                                                         | build (one key-press test)                                                                                                                                                                                                                        |
| `keymap:vscode-windows`, `module:src/platform/keymaps/vscode-windows.ts`                                                                                                                                                                                      | derived from macOS with Meta→Ctrl: `Ctrl+N`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | loaded (`keymaps.json:7352`), no handler                                                                                                                                                                       | build (one key-press test)                                                                                                                                                                                                                        |
| `ipc:light_edit_open_untitled`                                                                                                                                                                                                                                | `src/platform/project.ts:89-92` (`openLightEditUntitled`), called only from `windows.ts:506` when the window has no project and no LightEdit (Welcome). Adapter `src-tauri/src/commands/app.rs:126-148` → `AppState::open_light_empty` (`src-tauri/src/state.rs:174-177`) → `Host::open_light_empty` (`crates/ide-host/src/project.rs:725-728`), rooted at `dirs::home_dir()`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | `git grep -n open_light_empty HEAD -- app-gpui`: nothing                                                                                                                                                       | build (it needed a seam in `crates/ide-host/src/project.rs`, `open_light_empty_in(window, dir)`, because `open_light_empty` roots the LightEdit project at the real home folder (`project.rs:726`) and no test may do that; the seam is hunk 1.4) |
| `ipc:doc_set_bom`                                                                                                                                                                                                                                             | `backend.ts:95`; one call site `src/editor/core/view/StatusItems.tsx:385-403` (`applyBom`): the encoding item's menu shows, for a UTF-8 document that is not a native-to-ASCII `.properties` file (`StatusItems.tsx:381`), a separator and "Add UTF-8 BOM" / "Remove UTF-8 BOM", disabled when read-only (`:436-445`); the answer sets encoding/BOM and dirty; under autosave the document is saved at once (`:393`); a refusal is an error notification "Cannot change the UTF-8 BOM" (`:396-401`). Save writes the BOM (`crates/ide-workspace/src/documents.rs` `set_bom` → `save_text(…, has_bom)`)                                                                                                                                                                                                                                                                                                              | `editor/view.rs:1320-1344` has `set_encoding` only; `workbench/mod.rs:2253-2290` (`encoding_menu`) has no BOM row; no `doc_set_bom` call                                                                       | build                                                                                                                                                                                                                                             |
| `point.registerStatusItem:editor.encoding` (partial)                                                                                                                                                                                                          | label `{encoding} BOM` when the file has one (`StatusItems.tsx:419`); menu BOM row (above); Reload with Encoding disabled when dirty OR `!canReloadEncoding` (`:431`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | label already native: `workbench/status_items.rs:129` renders `{name} BOM` (the parity clause calling it missing is stale for this sub-item); BOM row missing; Reload disabled only when dirty (`mod.rs:2285`) | build (BOM row, canReloadEncoding)                                                                                                                                                                                                                |
| `ipc:doc_set_encoding` (partial)                                                                                                                                                                                                                              | missing clause: the BOM entry and Reload disabled when the document cannot be re-read                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | as above                                                                                                                                                                                                       | build (the same two sub-items)                                                                                                                                                                                                                    |
| `ipc:doc_sync_from_disk`                                                                                                                                                                                                                                      | `backend.ts:91`; one call site `controller.ts:466-472` → `installDiskSync` (`src/editor/core/diskSync.ts:5-51`): on window focus, debounced 200 ms, only with a project or LightEdit, not while opening or while a close is decided, `files.syncOnFocus !== false`; a check whose project changed is dropped; a focus during a running check runs one follow-up; after a check in a project (not LightEdit) `project.refresh` and `vcs.refresh` run. What the check does is the backend's: `Documents::sync_from_disk` (`crates/ide-workspace/src/documents.rs:1190-1192` → `reload_from_disk`, `:1218-1290`): a clean document whose file changed reloads; a dirty one is left and a warning `Notify` "{name} changed on disk" is raised; a missing file is skipped (`metadata` fails → `Ok(())`)                                                                                                                  | `editor_model/disk_sync.rs` is the debounce model with 6 tests; `git grep -n "disk_sync::\|DiskSync" HEAD -- app-gpui/src` finds no use outside the file; no `ops::doc::sync_from_disk` call                   | build                                                                                                                                                                                                                                             |
| `module:src/editor/core/diskSync.ts`                                                                                                                                                                                                                          | as above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | the model only                                                                                                                                                                                                 | build                                                                                                                                                                                                                                             |
| `unit:src/editor/core/diskSync.test.ts` (partial, 6 of 7)                                                                                                                                                                                                     | the missing case: "controller preserves the unchanged document's selection and scroll, then stops checking" (`diskSync.test.ts:143-179`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | 6 model tests in `editor_model/disk_sync.rs`                                                                                                                                                                   | build (the 7th through a window)                                                                                                                                                                                                                  |
| `unit:src/editor/core/view/statusItems.test.tsx` (no entry, 9 cases)                                                                                                                                                                                          | the untitled case: breadcrumbs of `untitled:Untitled-1` are the name alone                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Hypothesis (from the design note, not re-read): `editor_model/nav_bar.rs` would push a folder segment                                                                                                          | not in this slice: the other 8 cases are other features; listed so the row owner knows one case depends on untitled tabs                                                                                                                          |
| Welcome "New File" (`src/features/welcome/ProjectsPage.tsx:36`, `:90`) and the LightEdit empty editor's "New File" hint (`src/workbench/EditorArea.tsx:20-24`)                                                                                                | both run `file.newUntitled`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | no button / hint (`workbench/empty_editor.rs` has Open File only)                                                                                                                                              | build the hint (hunk 1.9); the Welcome buttons belong to the welcome rows' owner and only need the action                                                                                                                                         |
| `src/features/app/__tests__/windowClose.test.ts:118` "stays open when an untitled buffer's Save As is cancelled", `src/features/app/__tests__/closeGuard.test.ts:285` "counts untitled buffers only when leaving for Welcome, and stays when one isn't saved" | `closeGuard.ts:242-317`: window close / Quit / leaving count untitled buffers; Save runs Save As for each; one left unsaved keeps the window open and reports nothing; opening another local project in place neither counts nor saves them (they follow the window)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | `CloseDecision::saved(Err)` always reports (`app_models/close_guard.rs`), the leave paths share `request_leave`                                                                                                | build for the counting and Save As-or-stay part; the carry into another project is not built: see (c)                                                                                                                                             |

Corrections to the brief and to `ipc_rows_read_1.md` found while reading:

- "Save As on an existing file's tab": the webview has none (no action, no menu entry); `ipc_rows_read_1.md:637` "File › Save As" is wrong.
- "a modified tab's conflict question, a deleted file's tab" for `doc_sync_from_disk`: the backend asks nothing. A dirty document gets a warning notification, and a deleted file is skipped by the sync (the deleted file's tab is closed by the watcher path, `controller.ts:1896-1915` `onFilesRemoved`, a different feature).
- `point.registerStatusItem:editor.encoding`: the `{name} BOM` label is native already (`workbench/status_items.rs:129`).

Totals per verdict (the brief's four), by parity key: build 15 (`action:file.newUntitled`, `ipc:doc_open_untitled`, `ipc:doc_save_as`, `ipc:light_edit_open_untitled` — its `needs`, the `crates/` seam, is supplied as hunk 1.4 —, `unit:src/editor/core/untitled.test.ts`, `keymap:vscode-macos`, `keymap:vscode-windows`, `module:src/platform/keymaps/vscode-macos.ts`, `module:src/platform/keymaps/vscode-windows.ts`, `ipc:doc_set_bom`, `point.registerStatusItem:editor.encoding`, `ipc:doc_set_encoding`, `ipc:doc_sync_from_disk`, `module:src/editor/core/diskSync.ts`, `unit:src/editor/core/diskSync.test.ts`); already there 0 rows (1 sub-item: the `{name} BOM` label); needs 0 left; cannot be tested 0 rows (1 sub-case: the save dialog's file-type list, (d) 1). Outside these: `unit:src/editor/core/view/statusItems.test.tsx` (one untitled case, not built here) and the two close-guard spec cases (one covered, one half).

## (b) Hunks for existing files, by piece

Three pieces, in this order. Piece 2 uses `Editor::can_reload_encoding` and `Editor::set_bom` from piece 1's `editor/view/untitled.rs`. Piece 3 stands alone. Every new file loses `_pending` when it is declared:

- `app-gpui/src/editor_model/untitled_pending.rs` → `untitled.rs`
- `app-gpui/src/editor/view/untitled_pending.rs` → `untitled.rs`
- `app-gpui/src/workbench/untitled_tabs_pending.rs` → `untitled_tabs.rs`
- `app-gpui/src/workbench/encoding_bom_pending.rs` → `encoding_bom.rs` (piece 2)
- `app-gpui/src/workbench/disk_sync_window_pending.rs` → `disk_sync_window.rs` (piece 3)
- `app-gpui/src/workbench/tests/proofs48_untitled_pending.rs` → `proofs48_untitled.rs`, `…/proofs48_save_as_pending.rs` → `proofs48_save_as.rs`, `…/proofs48_light_edit_untitled_pending.rs` → `proofs48_light_edit_untitled.rs` (piece 1), `…/proofs48_bom_pending.rs` → `proofs48_bom.rs` (piece 2), `…/proofs48_disk_sync_pending.rs` → `proofs48_disk_sync.rs` (piece 3)

### Piece 1: untitled buffers, Save As, the Welcome path

1.1 `app-gpui/src/editor_model/mod.rs`, anchor:

```
pub mod semantic_tokens;
pub mod virtual_input;
```

becomes

```
pub mod semantic_tokens;
pub mod untitled;
pub mod virtual_input;
```

1.2 `app-gpui/src/editor/view.rs`, three hunks.

(a) anchor `mod string_literals;\nmod token_style;` becomes

```
mod string_literals;
mod token_style;
mod untitled;
```

(b) the field, anchor (inside `pub struct Editor`):

```
    has_bom: bool,
    editorconfig: Option<EditorConfigProps>,
```

becomes

```
    has_bom: bool,
    /// Whether Reload with Encoding can read the document's source again ([`DocumentInfo::can_reload_encoding`]).
    can_reload_encoding: bool,
    editorconfig: Option<EditorConfigProps>,
```

(c) the initializer in `with_text`, anchor `            has_bom: info.has_bom,` becomes

```
            has_bom: info.has_bom,
            can_reload_encoding: info.can_reload_encoding,
```

1.3 `app-gpui/src/editor/view/save_cleanup.rs`: Save As runs the same cleanup as Save. Anchor `    fn clean_up_before_save(&mut self, cx: &mut Context<Self>) {` becomes `    pub(super) fn clean_up_before_save(&mut self, cx: &mut Context<Self>) {`.

1.4 `crates/ide-host/src/project.rs` (the seam; nothing else under `crates/` changes). Anchor:

```
    pub fn open_light_empty(&self, window: &str) -> IdeResult<Arc<Project>> {
        let dir = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
        self.open_light_in(window, dir, None)
    }
```

becomes

```
    pub fn open_light_empty(&self, window: &str) -> IdeResult<Arc<Project>> {
        let dir = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
        self.open_light_empty_in(window, dir)
    }

    /// [`Host::open_light_empty`] rooted at `dir`: the app passes the home folder, or the home of its
    /// own when it runs isolated, so that nothing indexes or watches the user's.
    pub fn open_light_empty_in(&self, window: &str, dir: PathBuf) -> IdeResult<Arc<Project>> {
        self.open_light_in(window, dir, None)
    }
```

1.5 `app-gpui/src/workbench/mod.rs`, in this order.

(a) anchor `mod todo_window;` becomes `mod todo_window;\nmod untitled_tabs;`.

(b) anchor `    editor_core::register_actions(registry);` becomes

```
    editor_core::register_actions(registry);
    untitled_tabs::register_actions(registry);
```

(c) tab titles and the Switcher's titles: the line `                    .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).to_owned());` occurs twice (in `tab_titles_of` and `open_editor_tabs`); replace both with

```
                    .unwrap_or_else(|| crate::editor_model::untitled::display_name(path).to_owned());
```

(d) `tabs_changed`, the close question of an untitled buffer. Anchor:

```
            if !editor.read(cx).is_dirty() {
                continue;
            }
            closing.push(editor.clone());
```

becomes

```
            if !editor.read(cx).is_dirty() {
                continue;
            }
            // An untitled buffer has no file to write itself to: closing the last tab of one with text
            // always asks, whatever autosave says; while another tab of it is open that tab keeps the text.
            if editor.read(cx).is_untitled() {
                let doc = editor.read(cx).doc();
                if !editors.values().any(|other| other.read(cx).doc() == doc) {
                    closing.push(editor.clone());
                    untitled_tabs::ask_closing_untitled(editor, window, cx);
                }
                continue;
            }
            closing.push(editor.clone());
```

(e) `restore_unsaved`, the tab that comes back keeps its kind. Anchor:

```
        let input = EditorInput { kind: "file".to_owned(), key: path.clone(), path: Some(path), data: None };
```

becomes

```
        let input = if editor.read(cx).is_untitled() {
            crate::editor_model::untitled::untitled_input(&path)
        } else {
            EditorInput { kind: "file".to_owned(), key: path.clone(), path: Some(path), data: None }
        };
```

(f) `autosave`: anchor

```
        let unsaved: Vec<_> = editors.into_iter().filter(|editor| editor.read(cx).is_dirty()).collect();
```

becomes

```
        // An untitled buffer has no file to write itself to: only an explicit save gives it one.
        let unsaved: Vec<_> = editors
            .into_iter()
            .filter(|editor| editor.read(cx).is_dirty() && !editor.read(cx).is_untitled())
            .collect();
```

(g) `save_documents`: anchor

```
        let unsaved: Vec<_> =
            self.document_editors(cx).into_iter().filter(|editor| editor.read(cx).is_dirty()).collect();
```

becomes

```
        // Untitled buffers are written through Save As (`untitled_tabs::save_untitled_buffers`).
        let unsaved: Vec<_> = self
            .document_editors(cx)
            .into_iter()
            .filter(|editor| editor.read(cx).is_dirty() && !editor.read(cx).is_untitled())
            .collect();
```

(h) `save_all_documents` takes the window and asks for the untitled buffers' files. Anchor:

```
    pub(crate) fn save_all_documents(&mut self, cx: &mut Context<Self>) {
        let saving = self.save_documents(cx);
        cx.spawn(async move |workbench, cx| {
            let Some(failure) = saving.await else { return };
```

becomes

```
    pub(crate) fn save_all_documents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saving = self.save_documents(cx);
        cx.spawn_in(window, async move |workbench, cx| {
            let failure = saving.await;
            // Then each untitled buffer with text asks where it goes, one dialog after the other.
            if let Err(error) = untitled_tabs::save_untitled_buffers(&workbench, cx).await {
                tracing::warn!("the untitled buffers were not saved: {error:#}");
            }
            let Some(failure) = failure else { return };
```

Also update its doc comment's first line to `/// File › Save All for the text documents, Save As for each untitled buffer with text (the notebooks…` if the integrator wants it to say so.

(i) `title`: anchor

```
            .or_else(|| self.active_tab_view().and_then(|view| view.input.path.as_deref()).map(PathBuf::from));
        let default = cx
```

becomes

```
            .or_else(|| self.active_tab_view().and_then(|view| view.input.path.as_deref()).map(PathBuf::from))
            // An untitled buffer is named, not a path: `untitled:Untitled-1` is titled Untitled-1.
            .map(|file| match file.to_str() {
                Some(path) if crate::editor_model::untitled::is_untitled_path(path) => {
                    PathBuf::from(crate::editor_model::untitled::untitled_name(path))
                }
                _ => file,
            });
        let default = cx
```

(j) `reopen_closed_tab`: anchor `            .filter(|tab| tab.input.kind == tab_views::FILE_KIND)` becomes `            .filter(|tab| crate::editor_model::untitled::is_text_kind(&tab.input.kind))`.

(k) `load_active_tab`: anchor `            if tab.input.kind == tab_views::FILE_KIND {` becomes `            if crate::editor_model::untitled::is_text_kind(&tab.input.kind) {`.

(l) `save_session`: anchor

```
        let mut files = restored_views.clone();
        for editor in editors.values() {
            let editor = editor.read(cx);
```

becomes

```
        let mut files = restored_views.clone();
        for editor in editors.values() {
            let editor = editor.read(cx);
            // An untitled buffer is never restored: it has no place to keep.
            if editor.is_untitled() {
                continue;
            }
```

(m) `load_tab_probed`: the wiring of a new editor moves to `Workbench::install_tab_editor` (`untitled_tabs.rs`), which a new untitled buffer's tab uses too. Replace this block (copied from HEAD; it is the whole `Ok((info, version, rope))` arm):

```
                    Ok((info, version, rope)) => {
                        let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                        if still_open {
                            workbench.install_intelligence(&editor, window, cx);
                        }
                        // A tab closed while its file was opening has no use for the editor;
                        // dropping it releases the document.
                        if let (
                            true,
                            Content::Project { editors, restored_views, .. }
                            | Content::Editor { editors, restored_views, .. },
                        ) = (still_open, &mut workbench.content)
                        {
                            // The file is where it was left when the project was last open.
                            let path = editor.read(cx).path().to_string_lossy().into_owned();
                            if let Some(view) = restored_views.remove(&path) {
                                editor.update(cx, |editor, cx| {
                                    editor.select(view.anchor, view.head, cx);
                                    if let Some(line) = view.top_line {
                                        editor.scroll_to_line(line, cx);
                                    }
                                });
                            }
                            let focused = tab_id.clone();
                            let took_focus =
                                cx.on_focus_in(&editor.focus_handle(cx), window, move |workbench, window, cx| {
                                    workbench.editor_took_focus(&focused, window, cx);
                                });
                            editors.insert(tab_id.clone(), editor.clone());
                            if let Content::Project { editor_focus, .. } | Content::Editor { editor_focus, .. } =
                                &mut workbench.content
                            {
                                editor_focus.insert(tab_id.clone(), took_focus);
                            }
                            workbench.wrap_markdown(&tab_id, &editor, window, cx);
                        }
                        workbench.serve_reveal(cx);
                        if still_open {
                            workbench.watch_debug_editor(&editor, window, cx);
                            workbench.watch_plugin_gutter(&editor, window, cx);
                            workbench.http_editor_opened(&editor, window, cx);
                            workbench.editor_gutter_opened(&editor, window, cx);
                        }
                    }
```

with

```
                    Ok((info, version, rope)) => {
                        let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                        workbench.install_tab_editor(&tab_id, editor, still_open, window, cx);
                    }
```

(n) the test modules, anchor `    mod proofs44_editor_tabs;` becomes

```
    mod proofs44_editor_tabs;
    mod proofs48_light_edit_untitled;
    mod proofs48_save_as;
    mod proofs48_untitled;
```

1.6 `app-gpui/src/editor/actions.rs`: anchor `        workbench.update(cx, |workbench, cx| workbench.save_all_documents(cx));` becomes `        workbench.update(cx, |workbench, cx| workbench.save_all_documents(window, cx));`.

1.7 `app-gpui/src/workbench/tab_views.rs`, `open_tab_file`: a tab of an untitled buffer opens it again by its key (one more reference for the new editor). Anchor:

```
pub(super) async fn open_tab_file(host: &Host, label: &str, path: PathBuf, probe: bool) -> IdeResult<TabFile> {
    if probe {
```

becomes

```
pub(super) async fn open_tab_file(host: &Host, label: &str, path: PathBuf, probe: bool) -> IdeResult<TabFile> {
    let key = path.to_string_lossy();
    if crate::editor_model::untitled::is_untitled_path(&key) {
        let key = key.into_owned();
        return ide_host::ops::doc::open_untitled(host, label, Some(key)).await.map(TabFile::Text);
    }
    if probe {
```

1.8 `app-gpui/src/workbench/light_edit_tabs.rs`, three hunks.

(a) `light_edit_content`: anchor

```
        let input = EditorInput { kind: FILE_KIND.to_owned(), key: path.clone(), path: Some(path.clone()), data: None };
```

becomes

```
        // The first tab of a window that turned into LightEdit for File › New is an untitled buffer's.
        let input = if editor.read(cx).is_untitled() {
            crate::editor_model::untitled::untitled_input(&path)
        } else {
            EditorInput { kind: FILE_KIND.to_owned(), key: path.clone(), path: Some(path.clone()), data: None }
        };
```

(b) `light_edit_title`: anchor `        Some(path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned())` becomes `        Some(crate::editor_model::untitled::display_name(&path).to_owned())`.

(c) `save_light_session`: anchor

```
        for editor in editors.values() {
            let editor = editor.read(cx);
            let view =
```

(unique in this file) becomes

```
        for editor in editors.values() {
            let editor = editor.read(cx);
            if editor.is_untitled() {
                continue;
            }
            let view =
```

(d) the existing test `an_empty_light_edit_pane_offers_open_file_and_dropping` (see (e)): anchor

```
        assert_eq!(shown, [("Open File", Some("file.openFile")), ("Drop files here to open them", None)]);
```

becomes

```
        assert_eq!(
            shown,
            [
                ("New File", Some("file.newUntitled")),
                ("Open File", Some("file.openFile")),
                ("Drop files here to open them", None)
            ]
        );
```

and its doc comment and name say New File too: `an_empty_light_edit_pane_offers_new_file_open_file_and_dropping`.

1.9 `app-gpui/src/workbench/empty_editor.rs`: anchor

```
const LIGHT_EDIT_HINTS: [Hint; 2] = [
    Hint { label: "Open File", action: Some("file.openFile"), fallback: None },
```

becomes

```
const LIGHT_EDIT_HINTS: [Hint; 3] = [
    Hint { label: "New File", action: Some("file.newUntitled"), fallback: None },
    Hint { label: "Open File", action: Some("file.openFile"), fallback: None },
```

and the comment above it, `/// A window that edits loose files has no project to navigate: only opening files works there.`, becomes `/// A window that edits loose files has no project to navigate: only opening and creating files works there.`

1.10 `app-gpui/src/workbench/menu_spec.rs` (the native File menu has no New submenu; the entry goes beside the other New rows): anchor `        act("file.newScratchFile", "New Scratch File…"),` becomes

```
        act("file.newScratchFile", "New Scratch File…"),
        act("file.newUntitled", "New Untitled File"),
```

(`app.action.newFile` is "New Untitled File"; the webview's submenu row says "Untitled File" under New, `src-tauri/src/menu.rs:229`; in a flat menu the action's title reads right.)

1.11 `app-gpui/src/app_models/close_guard.rs`: anchor

```
    /// Save All finished.
    pub fn saved(&mut self, result: Result<(), String>) -> CloseStep {
```

becomes

```
    /// Save All ended with an untitled buffer still unsaved (its Save As was cancelled or refused, which
    /// was told already): the window stays and nothing is reported.
    pub fn save_cancelled(&mut self) -> CloseStep {
        self.decided(false, None)
    }

    /// Save All finished.
    pub fn saved(&mut self, result: Result<(), String>) -> CloseStep {
```

Add a unit test beside the module's tests if the integrator wants one: `save_cancelled` → `CloseStep::Decided { close: false, failure: None, .. }`.

1.12 `app-gpui/src/workbench/app_shell.rs`, `decide`: anchor

```
            CloseStep::SaveAll => {
                let saving = this.update(cx, |workbench, cx| workbench.save_all(cx))?;
                decision.saved(saving.await)
            }
```

becomes

```
            CloseStep::SaveAll => {
                let saving = this.update(cx, |workbench, cx| workbench.save_all(cx))?;
                match saving.await {
                    // Then each untitled buffer with text asks for its file; one left without a file keeps
                    // the window, and nothing failed.
                    Ok(()) => match super::untitled_tabs::save_untitled_buffers(this, cx).await? {
                        true => decision.saved(Ok(())),
                        false => decision.save_cancelled(),
                    },
                    failed => decision.saved(failed),
                }
            }
```

1.13 `app-gpui/src/workbench/editor_core.rs`: Reload from Disk / Reload All from Disk leave untitled buffers alone (the backend refuses to reload one, `crates/ide-workspace/src/documents.rs:1063-1066` "save the file before reloading it with another encoding", which would be told as "Cannot reload from disk"). Anchor in `file.reloadAllFromDisk`:

```
                .filter(|editor| !editor.is_dirty())
```

becomes

```
                .filter(|editor| !editor.is_dirty() && !editor.is_untitled())
```

and in `reload_from_disk`, anchor

```
            let Some(editor) = editors.iter().find(|editor| editor.read(cx).path().to_string_lossy() == **path) else {
                continue;
            };
```

becomes

```
            let Some(editor) = editors.iter().find(|editor| editor.read(cx).path().to_string_lossy() == **path) else {
                continue;
            };
            // An untitled buffer has no file to read again.
            if editor.read(cx).is_untitled() {
                continue;
            }
```

1.14 `app-gpui/src/workbench/tab_views.rs`, `open_text_tab`: Back / Forward / Last Edit Location (`editor_core.rs:219-223` → `open_project_file_at`) and Recent Files can open a recorded `untitled:` key again; its tab must keep the untitled kind, or the session would store it as a file. Anchor:

```
                let input = EditorInput { kind: FILE_KIND.to_owned(), key: key.clone(), path: Some(key), data: None };
```

becomes

```
                let input = if crate::editor_model::untitled::is_untitled_path(&key) {
                    crate::editor_model::untitled::untitled_input(&key)
                } else {
                    EditorInput { kind: FILE_KIND.to_owned(), key: key.clone(), path: Some(key), data: None }
                };
```

Hypothesis: with this, reopening the key of a buffer that was closed gives a new empty buffer of that name (`open_untitled(Some(key))` mints it, `documents.rs:545-595`), as the webview's `acquire` (`document.ts:833`) does. Check: Back after closing an untitled tab with Don't Save.

### Piece 2: the byte order mark and Reload with Encoding

2.1 `app-gpui/src/workbench/mod.rs`:

(a) anchor `pub mod empty_editor;\nmod github;` becomes

```
pub mod empty_editor;
mod encoding_bom;
mod github;
```

(b) `StatusCommand`: anchor

```
    /// Keep the text and save it in this encoding.
    Convert(String),
}
```

becomes

```
    /// Keep the text and save it in this encoding.
    Convert(String),
    /// Add (`true`) or remove the UTF-8 byte order mark.
    Bom(bool),
}
```

(c) `encoding_menu`: anchor

```
        vec![
            // Reloading would lose what is not saved; a read-only file cannot be written in another encoding.
            group("Reload with Encoding", dirty, others(true)),
            group("Convert to Encoding", status.read_only, others(false)),
            MenuItem::Separator,
            MenuItem::action_with("file.settings", "editor.fileEncodings".into()).labelled("File Encodings Settings…"),
        ]
```

becomes

```
        // Reloading would lose what is not saved and needs a source to read again; a read-only file
        // cannot be written in another encoding.
        let mut rows = vec![
            group("Reload with Encoding", dirty || !editor.can_reload_encoding(), others(true)),
            group("Convert to Encoding", status.read_only, others(false)),
        ];
        rows.extend(self.bom_rows(editor, &status, cx));
        rows.push(MenuItem::Separator);
        let settings = MenuItem::action_with("file.settings", "editor.fileEncodings".into());
        rows.push(settings.labelled("File Encodings Settings…"));
        rows
```

and the doc comment `/// The File Encoding item's menu: reload the file in another encoding, or convert it to one.` becomes `/// The File Encoding item's menu: reload the file in another encoding, convert it to one, or add or remove a UTF-8 file's byte order mark.`

(d) `run_status_command`: anchor

```
            StatusCommand::Reload(encoding) => ("Cannot reload with encoding", true, encoding),
```

becomes

```
            StatusCommand::Bom(has_bom) => {
                self.change_bom(&editor, has_bom, cx);
                return;
            }
            StatusCommand::Reload(encoding) => ("Cannot reload with encoding", true, encoding),
```

(e) tests: after the piece-1 line `    mod proofs48_untitled;` add `    mod proofs48_bom;` (alphabetically before `proofs48_light_edit_untitled`; order does not matter).

### Piece 3: Synchronize files on frame activation

3.1 `app-gpui/src/workbench/mod.rs`:

(a) anchor `mod diff_window;\nmod edit_actions;` becomes

```
mod diff_window;
mod disk_sync_window;
mod edit_actions;
```

(b) the field, anchor

```
    idle_save: Option<gpui_kit::Task<()>>,
```

becomes

```
    idle_save: Option<gpui_kit::Task<()>>,
    /// Synchronize files on frame activation: the debounced disk check of the window ([`disk_sync_window`]).
    disk_sync: disk_sync_window::DiskSyncSlot,
```

(c) the initializer, anchor `            idle_save: None,` becomes

```
            idle_save: None,
            disk_sync: Default::default(),
```

(d) the activation, anchor

```
            // Another window may have opened or forgotten a project meanwhile.
            if window.is_window_active() {
                workbench.sync_welcome(window, cx);
            }
```

becomes

```
            // Another window may have opened or forgotten a project meanwhile; files may have changed on
            // disk while the window was in the background.
            if window.is_window_active() {
                workbench.sync_welcome(window, cx);
                workbench.disk_sync_focused(window, cx);
            }
```

(e) tests: `    mod proofs48_disk_sync;` beside the other `proofs48_` lines.

IPC commands each action ends up calling, for the parity records: `file.newUntitled` → `doc_open_untitled` (`ide_host::ops::doc::open_untitled`, as `src-tauri/src/commands/doc.rs:38`), and from a window showing nothing `light_edit_open_untitled` (`Host::open_light_empty_in`, the same `open_light_in` the adapter's `AppState::open_light_empty` reaches; the adapter's detach of auxiliary windows applies only when a non-LightEdit project is open, which a window showing nothing has not); `file.save`, the tab-close question's Save and the close decision's Save All → `doc_save_as` (`ide_host::ops::doc::save_as`, as `doc.rs:51`); the encoding menu's BOM row → `doc_set_bom` (`ide_host::ops::doc::doc_set_bom`, as `doc.rs:136`); window activation → `doc_sync_from_disk` (`ide_host::ops::doc::sync_from_disk`, as `doc.rs:91`). All five are allowed in Restricted Mode (`crates/ide-host/src/trust.rs:328-340`, `:512`), and the webview gates none of them with a `when` clause, so no trust check is added.

## (c) The state each row reaches, with its tests

Test paths: `workbench::tests::proofs48_untitled::…` (U), `workbench::tests::proofs48_save_as::…` (S), `workbench::tests::proofs48_light_edit_untitled::…` (L), `workbench::tests::proofs48_bom::…` (B), `workbench::tests::proofs48_disk_sync::…` (D), `editor_model::untitled::tests::…` (M).

| Row                                                                               | State after the slice                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Tests                                                                                                                                                                                                                                                                                                         |
| --------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:file.newUntitled`                                                         | done                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | U `opens_editable_pathless_buffers_with_incrementing_names` (by the registry in a project window: the tab, its kind and name, plain text, editable, unsaved after typing, Untitled-2 next, the window title), L `new_untitled_file_in_a_window_showing_nothing_turns_it_into_light_edit` (the Welcome branch) |
| `ipc:doc_open_untitled`                                                           | done: both webview sites (mint: U `opens_editable_pathless_buffers_with_incrementing_names`; re-acquire by key for a second tab: U `reuses_the_buffer_for_a_re_opened_untitled_name_and_shares_it_across_tabs`, which asserts the backend's buffer text through both editors)                                                                                                                                                                                                                                                                                        | as listed                                                                                                                                                                                                                                                                                                     |
| `ipc:doc_save_as`                                                                 | done for the one site: S `autosave_never_writes_an_untitled_buffer_explicit_save_asks_for_a_path_and_retargets_tab_and_model` (file on disk, tab and editor retargeted, language), S `save_as_onto_a_file_open_in_another_tab_is_refused_and_keeps_the_buffer` ("Cannot save file"), U `asks_about_a_dirty_untitled_buffer_when_its_last_tab_closes` (Save from the close question), S `stays_open_when_an_untitled_buffers_save_as_is_cancelled` (from the close decision). Missing: the save dialog's file-type list (Text files / languages / All files), see (d) | as listed                                                                                                                                                                                                                                                                                                     |
| `ipc:light_edit_open_untitled`                                                    | done: L `new_untitled_file_in_a_window_showing_nothing_turns_it_into_light_edit` (LightEdit project rooted at the test's home, the tab, the keyboard, Save As there)                                                                                                                                                                                                                                                                                                                                                                                                 | as listed                                                                                                                                                                                                                                                                                                     |
| `keymap:vscode-macos`, `module:src/platform/keymaps/vscode-macos.ts`              | done (the Missing clause is answered)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | U `the_vs_code_keymaps_open_an_untitled_buffer_with_cmd_n_and_ctrl_n`                                                                                                                                                                                                                                         |
| `keymap:vscode-windows`, `module:src/platform/keymaps/vscode-windows.ts`          | done                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | the same test                                                                                                                                                                                                                                                                                                 |
| `unit:src/editor/core/untitled.test.ts`                                           | partial, 6 of 7 by name plus half of the 7th: see the case table below                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |                                                                                                                                                                                                                                                                                                               |
| `ipc:doc_set_bom`                                                                 | done: B `the_encoding_menu_adds_and_removes_the_utf8_bom_and_the_save_writes_it`, B `with_autosave_on_adding_the_bom_writes_the_file_at_once`, B `the_bom_row_is_only_for_utf8_and_disabled_for_a_read_only_file`                                                                                                                                                                                                                                                                                                                                                    | as listed                                                                                                                                                                                                                                                                                                     |
| `point.registerStatusItem:editor.encoding`                                        | done: the BOM label (native already, asserted in B `the_encoding_menu_adds_and_removes_the_utf8_bom_and_the_save_writes_it`), the BOM row, Reload disabled without a source (B `reload_with_encoding_is_disabled_for_a_document_that_cannot_be_read_again`), plus the existing three tests                                                                                                                                                                                                                                                                           | as listed                                                                                                                                                                                                                                                                                                     |
| `ipc:doc_set_encoding`                                                            | partial stays only for the virtual-document re-read (no native code opens a library or revision document as a tab); the BOM entry and Reload disabled without a source are now proved by the B tests                                                                                                                                                                                                                                                                                                                                                                 | as listed                                                                                                                                                                                                                                                                                                     |
| `ipc:doc_sync_from_disk`                                                          | done: D `a_file_changed_outside_the_project_is_read_again_when_the_window_is_active_again` (a file no watcher sees is read again on activation; with `files.syncOnFocus` off nothing runs), D `a_changed_file_with_unsaved_edits_keeps_them_and_a_warning_says_so`                                                                                                                                                                                                                                                                                                   | as listed                                                                                                                                                                                                                                                                                                     |
| `module:src/editor/core/diskSync.ts`                                              | done: the six model tests in `editor_model::disk_sync::tests` plus the D tests (the debounce, the project scope and the follow-up are the model's; the window feeds it on activation and runs the check and the refreshes)                                                                                                                                                                                                                                                                                                                                           | as listed                                                                                                                                                                                                                                                                                                     |
| `unit:src/editor/core/diskSync.test.ts`                                           | done, 7 of 7: the six model tests plus D `controller_preserves_the_unchanged_documents_selection_and_scroll_then_stops_checking`                                                                                                                                                                                                                                                                                                                                                                                                                                     | as listed                                                                                                                                                                                                                                                                                                     |
| `src/features/app/__tests__/windowClose.test.ts:118` (case of a partial spec row) | covered by S `stays_open_when_an_untitled_buffers_save_as_is_cancelled` (one window, not two: the webview case closes one of two windows; the close decision is per window natively, so one window shows the same behaviour)                                                                                                                                                                                                                                                                                                                                         |                                                                                                                                                                                                                                                                                                               |
| `src/features/app/__tests__/closeGuard.test.ts:285`                               | half: S `close_project_counts_an_untitled_buffer_and_stays_when_its_save_as_is_cancelled` proves "counts … when leaving for Welcome, and stays when one isn't saved"; NOT built: opening another local project in place neither counts nor saves untitled buffers and carries them into the new project (`project.rs:91-93`, `controller.ts:387-425`); natively they are counted and asked about on that leave too                                                                                                                                                   |                                                                                                                                                                                                                                                                                                               |

`src/editor/core/untitled.test.ts`, case by case:

| Case                                                                                                | Native test                                                                                                                                                                                                                                                                                                                                                                     |
| --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| opens editable, pathless buffers with incrementing names                                            | U `opens_editable_pathless_buffers_with_incrementing_names`                                                                                                                                                                                                                                                                                                                     |
| autosave never writes an untitled buffer; explicit save asks for a path and retargets tab and model | S `autosave_never_writes_an_untitled_buffer_explicit_save_asks_for_a_path_and_retargets_tab_and_model` (+ U `the_session_never_holds_an_untitled_buffer`)                                                                                                                                                                                                                       |
| offers the registered languages as save-as types                                                    | not covered: Hypothesis: the system chooser of the native toolkit takes no file-type list (`prompt_for_new_path(&dir, Some(name))` is the only form called in the crate); the name without an extension and the folder are M `save_as_opens_in_the_project_else_beside_the_last_used_file_else_at_home`, which is not the spec's sentence because it does not prove the filters |
| keeps the buffer open and dirty when the save dialog is cancelled                                   | S `keeps_the_buffer_open_and_dirty_when_the_save_dialog_is_cancelled`                                                                                                                                                                                                                                                                                                           |
| asks about a dirty untitled buffer when its last tab closes                                         | U `asks_about_a_dirty_untitled_buffer_when_its_last_tab_closes`                                                                                                                                                                                                                                                                                                                 |
| closes an untitled buffer without saving when the user declines                                     | U `closes_an_untitled_buffer_without_saving_when_the_user_declines`                                                                                                                                                                                                                                                                                                             |
| reuses the buffer for a re-opened untitled name and shares it across tabs                           | U `reuses_the_buffer_for_a_re_opened_untitled_name_and_shares_it_across_tabs`                                                                                                                                                                                                                                                                                                   |

`src/editor/core/diskSync.test.ts`, case by case (`editor_model::disk_sync::tests::…` is the model at HEAD):

| Case                                                                                    | Native test                                                                                     |
| --------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| debounces focus events and refreshes the project tree and VCS                           | `editor_model::disk_sync::tests::debounces_focus_events_and_refreshes_the_project_tree_and_vcs` |
| defaults on for older settings and does nothing when the setting is off                 | `…::defaults_on_for_older_settings_and_does_nothing_when_the_setting_is_off`                    |
| `it.each` setting / project / opening / closing / dispose (`diskSync.test.ts:68`)       | `…::cancels_pending_sync_on_changes` (one loop)                                                 |
| ignores welcome windows and synchronizes LightEdit without project refreshes            | `…::ignores_welcome_windows_and_synchronizes_light_edit_without_project_refreshes`              |
| coalesces focus during a slow check into a follow-up and removes its listener           | `…::coalesces_focus_during_a_slow_check_into_a_follow_up_and_removes_its_listener`              |
| does not refresh a replacement project when an old check finishes                       | `…::does_not_refresh_a_replacement_project_when_an_old_check_finishes`                          |
| controller preserves the unchanged document's selection and scroll, then stops checking | D `controller_preserves_the_unchanged_documents_selection_and_scroll_then_stops_checking`       |

Other tests written: U `closing_one_of_two_tabs_of_an_untitled_buffer_asks_nothing`, M `an_untitled_key_is_shown_by_its_name_and_a_file_by_its_file_name`, M `save_as_turns_every_tab_of_the_buffer_into_the_files_tab`.

What stays missing in this feature (not built, each data-safe):

- Untitled buffers following the window into another project opened in place (the webview carries them; natively the leave asks about them like any unsaved file, and Don't Save drops them).
- The save dialog's file-type list.
- The tab limit (Editor Tabs › Tab limit) is not applied when an untitled tab opens (`TabOpening` is private to `tab_views.rs`); Hypothesis: the webview applies it through `admitInput`.
- A buffer saved as `.md` or `.sql` does not get the Markdown layouts or the SQL console until its tab is opened again (`wrap_markdown` runs at install time only).
- The breadcrumbs (`editor_model/nav_bar.rs`) of an untitled buffer: Hypothesis (from the design note, not re-read): a folder segment `untitled:` would show; `statusItems.test.tsx:271` wants the name alone.
- The webview's Git operations save first through `src/features/vcs/ops.ts:116-132` → `editorService().saveAll()` without `skipUntitled` (`src/editor/core/controller.ts:2038-2040`), so a checkout, rebase or patch with a dirty untitled buffer opens a Save As dialog there (read, not run). Natively those callers use `Workbench::save_all`, which now skips untitled buffers (hunk 1.5 (g)): no dialog before a Git operation. Deliberate: the operation never touches an untitled buffer; to match the webview, call `untitled_tabs::save_untitled_buffers` after `save_all` in `branches.rs:530` and the other callers.

## (d) Hypotheses, each with the check that settles it

1. Hypothesis: `cx.prompt_for_new_path(&dir, Some(name))` takes no file-type list, so the "offers the registered languages as save-as types" case cannot be proved natively. Check: read `prompt_for_new_path` in the toolkit source the build fetched (not read here: the brief forbids the cargo registry).
2. Hypothesis: `simulate_new_path_selection` answers a dialog already pending, so every test calls it after the action (as `app_actions.rs:1890`). The save dialog of the close question opens only after `simulate_prompt_answer` plus `run_until_parked`; the tests do that. Check: the first run of U `asks_about_a_dirty_untitled_buffer_when_its_last_tab_closes`.
3. Hypothesis: the project's watcher does not see a file outside the project's folder opened in a project window (only LightEdit watches parents, `ops/doc.rs:6-16`). If it does, D `a_file_changed_outside_the_project_is_read_again_when_the_window_is_active_again` fails in its second half (the text becomes `one two three` with the setting off). Check: that run.
4. Hypothesis: `window.activate_window()` in a test fires `observe_window_activation` (deactivation does: `keymap_reference.rs:203`). Check: D tests.
5. Hypothesis: the backend marks a file without write permission read-only on open, so the BOM row is disabled (B `the_bom_row_is_only_for_utf8_and_disabled_for_a_read_only_file`); and a UTF-16LE file with its BOM is detected as `utf-16le`. Check: that test, or `ide_vfs` load code.
6. Hypothesis: `EditorIntelligence` reads the editor's path per request, so after Save As the language services see the new path. Check: `editor/intelligence.rs` `install`; a `.rs` Save As with the fake server fixture.
7. Hypothesis: `Editor::change_language` running in `retarget` before or after the backend's `DocLanguage` event changes the language once (it returns early on the same language, `editor/view/doc_tags.rs:94-97`: read). Check: S test asserts `rust`.
8. Hypothesis: the untitled tab's icon is the generic file icon in both apps (`file_icon_name("untitled:Untitled-1")` natively, `fileIconName("Untitled-1")` in the webview). Check: `theme/icons.rs:126`.
9. Hypothesis: on macOS the editor does not claim `ctrl-n` (next line) before the keymap's `file.newUntitled` in the vscode-windows preset. Check: U `the_vs_code_keymaps_open_an_untitled_buffer_with_cmd_n_and_ctrl_n`.
10. Hypothesis: `await` inside the match arm of hunk 1.12 compiles (it is in an arm body, not a guard).

11. Idle save (System Settings › Save files if the IDE is idle) runs `file.save` (`workbench/idle_save.rs:22-26`), which now opens a Save As sheet for a dirty untitled buffer after the idle delay. The webview does the same: `src/features/settings/appearanceRuntime.ts:254-260` runs `executeAction("file.save")` (read, not run). Kept for parity; the real-window check (g) looks at it.
12. Hypothesis: opening another project in place copies the window's untitled buffers into the new project (`crates/ide-host/src/project.rs:91-93`, `:733`) if the old editors have not released them yet; natively nothing shows them, so an orphan could hold the name Untitled-1 there. Check: after Don't Save and opening another project here, `documents.untitled_buffers()` of the new project is empty, and the next File › New is Untitled-1.
13. The Welcome path drops a late answer when the window shows something else by then; the minted buffer then stays referenced in the LightEdit project until it closes (that project is replaced by whatever the window opened, which closes it).

## (e) Existing tests whose expectations change

- `workbench::light_edit_tabs::tests::an_empty_light_edit_pane_offers_open_file_and_dropping`: New File comes first (`src/workbench/EditorArea.tsx:20-24`); replacement in hunk 1.8 (d).
- Every test that activates a window and then advances the clock (`wait_long`) now runs a disk check 200 ms later, followed by `project.refresh` and `vcs.refresh` in a project window (`src/editor/core/diskSync.ts:17-40`). No assertion found that this changes, but VCS-refresh-counting tests are the first to look at if one fails.

## (f) Tests most likely to fail first, and why

1. S `stays_open_when_an_untitled_buffers_save_as_is_cancelled` and S `close_project_counts_…`: the order prompt answer → `run_until_parked` → path answer must match when the decision task reaches `prompt_for_new_path`.
2. U `the_vs_code_keymaps_…`: replacing the `Keymap` global by assignment may not reach the window's key dispatch (the find-bar test does the same, `editor/find_bar.rs:1640-1650`); and hypothesis 9.
3. D tests: hypotheses 3 and 4, and the timer runs on the executor's clock, which only `wait_long` advances.
4. L test: `project.root` is the non-canonical temp path; the assertion compares canonical forms, but the saved tab path compares the non-canonical one.
5. B `the_bom_row_is_only_for_utf8_…`: hypothesis 5.
6. Every `listed(&root) == ["main.rs"]` assertion fails if opening a project writes anything into its folder (an `.idea`-like folder, a session file): then compare against the listing taken before the action instead.

## (g) What a look at the real window must check

- ⌘N (VS Code keymap) and File › New Untitled File open Untitled-1, typed text marks the tab, and the title bar says Untitled-1.
- Closing its tab asks "Untitled-1 has unsaved changes. Save it before closing?"; Save opens the system save sheet at the project root with the name Untitled-1; save into a temporary folder only, never into a real project.
- After Save As to `.rs`: highlighting switches to Rust, the gutter's VCS marks and the language server follow the new path (hypothesis 6).
- Two dirty untitled buffers and Save All: the sheets come one after the other.
- The encoding item of a UTF-8 file: Add UTF-8 BOM, label "UTF-8 BOM", saved bytes start EF BB BF.
- Change a file outside the project in a terminal while the window is in the background: it updates on coming back.
- With idle saving on, a dirty untitled buffer gets a Save As sheet after the idle delay (hypothesis 11).
- Welcome › File › New Untitled File (once the welcome button exists) becomes LightEdit without indexing the home folder (watch CPU and the background tasks).
