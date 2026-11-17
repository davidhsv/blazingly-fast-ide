# I-UNTITLED: untitled buffers in the native app (design note)

Everything here was read at HEAD `13c01811f` (branch `gpui`) with `git show HEAD:<path>` / `git grep … HEAD`. Line numbers are HEAD's. A statement not read from the code is marked Hypothesis. Nothing was compiled or run.

## (a) The webview's behaviour (checklist)

Creation and identity

- [ ] `file.newUntitled` action: `src/features/app/windows.ts:496-509`. Title key `app.action.newFile` ("New Untitled File"), category File, icon `plus`, keywords new/create/untitled/file/light edit. It returns early in a remote window (`desktopOnly()`, line 503). A window that has neither a project nor LightEdit (Welcome) first becomes a LightEdit window without a file (`openLightEditUntitled()`, `src/platform/project.ts:89-92`, IPC `light_edit_open_untitled`, `src-tauri/src/commands/app.rs:126-148` → `AppState::open_light_empty`), then the editor opens the buffer focused.
- [ ] Bindings: `src/platform/keymaps/vscode-macos.ts:198` `"file.newUntitled": ["Meta+N"]`; vscode-windows derives from it with Meta→Ctrl (`src/platform/keymaps/vscode-windows.ts:1-20`). The generated native keymaps already carry both: `app-gpui/assets/gen/keymaps.json:5464` (vscode-macos, `Meta+N`) and `:7352` (vscode-windows, `Ctrl+N`). The IntelliJ presets bind nothing to it.
- [ ] Where it is offered: File › New › Untitled File (`src-tauri/src/menu.rs:229`, and the File menu's toolbar/keymap list at `:2016`); Welcome's header button and landing action "New File" (`src/features/welcome/ProjectsPage.tsx:36`, `:90`); the empty LightEdit editor's hint "New File" (`src/workbench/EditorArea.tsx:20-24`).
- [ ] `EditorController.openUntitled` (`src/editor/core/controller.ts:1209-1218`): `backend.openUntitled(null)` mints a buffer, `manager.adopt(info)` registers its model, the tab opens with input `untitledInput(uri) = { kind: "untitled", key: uri, path: uri }` (`controller.ts:144`).
- [ ] The backend mints `untitled:Untitled-N` with the smallest free N (`crates/ide-workspace/src/documents.rs:597-605`); a closed buffer's number is reused (`crates/ide-workspace/tests/documents.rs:1094-1096`). Re-opening an existing key re-references the same buffer (`documents.rs:545-549`, `reopen` at `:607-617` increments `refs`). Two tabs (a split) share one model (`src/editor/core/untitled.test.ts`, case "reuses the buffer for a re-opened untitled name and shares it across tabs").
- [ ] A new buffer: empty, `plaintext`, editable, not dirty, encoding/BOM/line ending from the file defaults, `can_reload_encoding: false` (`documents.rs:545-595`; tests `documents.rs:1029-1044`, `:1104-1127`). Disk syncs never touch it (`documents.rs` test `:1122-1126`). Encoding conversion works, reload-with-encoding is refused (`:1117-1121`).
- [ ] `DocModel.virtual` is false for an untitled buffer (`src/editor/core/model/document.ts:242`): it is a normal editable text document in every respect except saving.

Title, icon, breadcrumbs, window title

- [ ] Tab title `Untitled-1` (`untitledTitle`, `src/platform/virtualDocs.ts:23-26`; `controller.ts:242`); tab icon `fileIconName("Untitled-1")` (`controller.ts:255`); sort key by that name (`controller.ts:212-214`).
- [ ] Breadcrumbs show the name alone, no folder (`src/editor/core/view/StatusItems.tsx:79-83`; spec `src/editor/core/view/statusItems.test.tsx:271-273`).
- [ ] LightEdit window title shows `Untitled-1` (`src/workbench/Workbench.tsx:46`).

Language

- [ ] No language detection on typing or paste: the buffer stays `plaintext` until Save As; the backend re-detects from the new path (`documents.rs:874-896`, emits `IdeEvent::DocLanguage` when it changed). The spec says so: "No content guessing: the bare name plus the type dropdown" (`untitled.test.ts`, case "autosave never writes…").

Save, Save As, Save All, autosave

- [ ] Autosave (reason `auto`) never writes an untitled buffer and opens no dialog (`document.ts:675-680`).
- [ ] An explicit save (Save All = `file.save`, `src/editor/core/actions.ts:1283-1288` → `ctrl.saveAll()` → `manager.saveAll("explicit")`, `document.ts:901-915`) runs `manager.saveAs(model)` for each dirty untitled buffer → `controller.saveUntitled` (`controller.ts:1225-1248`): a save dialog titled "Save As", `defaultPath` = `<dir>/Untitled-1` (no extension) where dir is the project root, else in LightEdit the folder of the most recently used file tab, else the bare name (`saveAsSuggestion`, `controller.ts:1255-1269`); filters = "Text files (_.txt)" first, every registered language sorted by name, "All files (_.*)" last (`languageFilters`, `controller.ts:172-194`).
- [ ] Cancel: the buffer stays untitled and dirty, nothing is written (spec case "keeps the buffer open and dirty when the save dialog is cancelled").
- [ ] Chosen path: `backend.saveAs(doc, path)` (`ide_host::ops::doc::save_as`, `crates/ide-host/src/ops/doc.rs:34-44`: in LightEdit it also starts watching the folder). The backend creates missing folders, writes `text_for_save` (the save cleanup's final newline applies before, `document.ts:660-672`), re-keys the same DocId (undo kept), re-detects language, clears `uri`, clears dirty (`documents.rs:818-899`). The model is retargeted (`document.ts:692-716`, `DocumentManager.retarget` `:809-814`) and every tab with the old key becomes `fileInput(path)` (`controller.ts:1272-1288`).
- [ ] Save As refused (target open as another document, or an `untitled:` target): an error notification "Cannot save file" with the message, the buffer stays untitled and dirty (`controller.ts:1239-1246`; backend `documents.rs:827-835`; test `documents.rs:1073-1101`).
- [ ] Hypothesis: the webview runs the Save As dialogs of several dirty untitled buffers concurrently (`Promise.all` in `document.ts:903-910`); how Tauri stacks them was not checked.
- [ ] Hypothesis: the webview's idle save (`src/features/settings/appearanceRuntime.ts:252-260`) runs `file.save`, which is an explicit Save All, so it would open a Save As dialog for a dirty untitled buffer after the idle delay. Not verified by a run.

Closing a tab

- [ ] Closing the last tab of a dirty untitled buffer always asks, whatever autosave says (`controller.ts:1609-1641`): title `app.unsaved.title`, body `app.unsaved.fileBody` with `{name}` = `Untitled-1`, buttons Don't Save (`app.unsaved.discard`) and Save (`app.unsaved.saveFile`, primary). Save runs the Save As flow; still dirty afterwards (dialog cancelled or save failed) keeps the tab. Don't Save closes it and writes nothing. Dismiss/Cancel keeps it. Closing one of two tabs of the same buffer asks nothing (`closedUntitledKeys`, `controller.ts:220-227`). Spec cases "asks about a dirty untitled buffer when its last tab closes" and "closes an untitled buffer without saving when the user declines".
- [ ] A clean untitled buffer closes silently.

Window close, Quit, leaving the project

- [ ] Window close and Quit (`src/features/app/closeGuard.ts:242-253`): the unsaved count includes the untitled buffers; Save runs Save All (Save As for each), then if any untitled buffer is still dirty the save fails as `cancelled` ("Untitled files were not saved", `closeGuard.ts:273-283`), the window stays and nothing is reported (`onSaveFailed` ignores cancelled, `closeGuard.ts:172-174`); Quit is cancelled (`commands.appCancelQuit`). Don't Save closes and the buffers are gone. Spec `src/features/app/__tests__/windowClose.test.ts:118` "stays open when an untitled buffer's Save As is cancelled".
- [ ] Opening another local project in place (`confirmLeave(…, keepUntitled = true)`, `closeGuard.ts:288-317`, `src/platform/project.ts:41-48`): untitled buffers are neither counted nor saved (Save All with `skipUntitled`), they follow the window. Close Project / leaving for Welcome / a remote host (`keepUntitled = false`) counts them and uses the Save-As-or-stay rule. Spec `src/features/app/__tests__/closeGuard.test.ts:285` "counts untitled buffers only when leaving for Welcome, and stays when one isn't saved".
- [ ] Following the window: the backend copies `(key, text, dirty)` of every open untitled buffer into the new project's registry (`crates/ide-host/src/project.rs:91-93`, `:633-635` for `open_project`; `:733-734`, `:815-817` for LightEdit); the controller keeps their tabs in the first pane, re-activates the one that was active and rebuilds their editors against the carried documents (`controller.ts:387-425`). Backend test `crates/ide-host/tests/light_edit.rs:221-255`.

Session

- [ ] The session never stores an untitled buffer: not its tab and not a caret entry (`controller.ts:613-614`; spec case "autosave never writes…" asserts `serializeSession` has no `untitled:`). After a restart they are gone (the close question had them).

What the backend knows (it is complete)

- [ ] `Documents::open_untitled(name: Option<&str>) -> IdeResult<DocumentInfo>` (`documents.rs:545`), `save_as(doc, target) -> IdeResult<DocumentInfo>` (`:823`), `is_untitled(doc) -> IdeResult<bool>` (`:1110`), `untitled_buffers() -> Vec<(String, String, bool)>` (`:1117`), `restore_untitled_buffer(key, text, dirty)` (`:1131`). `save(doc)` on an untitled buffer is an `Ok` no-op that leaves it dirty (`documents.rs` test `:1042-1044`; `crates/ide-host/tests/light_edit.rs:198-199`).
- [ ] Host ops: `ide_host::ops::doc::open_untitled(host, window, name: Option<String>)` (`crates/ide-host/src/ops/doc.rs:27`), `ops::doc::save_as(host, window, doc, path: PathBuf)` (`:34`), `Host::open_light_empty(window) -> IdeResult<Arc<Project>>` (`crates/ide-host/src/project.rs:725-728`, rooted at `dirs::home_dir()`). RPC names `doc_open_untitled` (`crates/ide-host/src/rpc.rs:222`), trust lists `doc_open_untitled` and `light_edit_open_untitled` (`crates/ide-host/src/trust.rs:328`, `:512`).
- [ ] LSP: `untitled:` is a virtual URI scheme (`crates/ide-lsp/src/uri.rs:258`), file operations register the `untitled` scheme (`crates/ide-lsp/src/file_ops.rs:76`).

## (b) The native state

- Nothing natively creates, opens or saves an untitled buffer: `git grep -n -i untitled HEAD -- app-gpui` finds only the generated manifests (`app-gpui/assets/gen/actions.json:3722`, `keymaps.json:5464`, `:7352`), a test of `is_virtual_uri` (`app-gpui/src/platform/virtual_docs.rs:187`), and unrelated "untitled" status items / New Project name. `file.newUntitled` has no handler, so the VS Code presets' Cmd/Ctrl+N does nothing.
- Tabs: `EditorInput { kind, key, path: Option<String>, data }` (`app-gpui/src/editor/tabs.rs:29-38`); `TabsState` has no "replace the input of every tab with key K" method (method list `tabs.rs:342-942`).
- A text tab is a tab whose `input.kind == "file"` (`app-gpui/src/workbench/tab_views.rs:48` `FILE_KIND`); its editor is loaded by path: `load_active_tab` (`app-gpui/src/workbench/mod.rs:3845-3876`) → `load_tab` / `load_tab_probed` (`mod.rs:3981-4100`) → `tab_views::open_tab_file` (`tab_views.rs:430-441`, an fs probe then `ops::doc::open`). `ops::doc::open` on an `untitled:` key would `mark_recent` it and, in LightEdit, `watch_light_dir("")` of its empty parent (`crates/ide-host/src/ops/doc.rs:6-16`); on a key that is not open it tries to read a file named `untitled:Untitled-1` (`documents.rs:383-390`). So an untitled tab must never go through `open_tab_file`.
- The editor: `Editor { path: PathBuf, language, read_only, line_ending, encoding, editorconfig, dirty, … }` (`app-gpui/src/editor/view.rs:319-360`), built by `Editor::with_text(host, window_label, &info, version, rope, cx)` (`view.rs:403-499`); it registers itself in `DocumentEditors` (doc → editors, `view.rs:413`), listens to `DocPatched`, `DocDirty`, `DocLanguage` (→ `change_language`), `SettingsChanged` (`view.rs:414-433`), and closes its document reference on release (`view.rs:440-452`). `path` never changes after construction. `Editor::save` calls `ops::doc::save` (`view.rs:1308-1311`), a no-op for an untitled buffer.
- Save All: `file.save` → `Workbench::save_all_documents` (`app-gpui/src/editor/actions.rs:63-68`, `mod.rs:1968-1980`) → `save_documents` saves every dirty editor (`mod.rs:1946-1964`). Close decisions use `Workbench::save_all` (`mod.rs:1983-1999`). `unsaved_count` counts dirty document editors (`mod.rs:1909-1911`). Autosave (`mod.rs:1921-1942`) saves dirty editors.
- Closing a tab with a dirty editor (`tabs_changed`, `mod.rs:1741-1800`): with `files.autoSave` on it saves without asking (`decision = (!auto_save).then(…)`, `mod.rs:1763`; `None => Some(0)`, `:1778-1781`); with it off a window prompt "Save All / Don't Save / Cancel" names the path (`mod.rs:1764-1774`). For an untitled buffer the autosave branch would "save" (a no-op `Ok`) and release the editor: the text would be lost. `restore_unsaved` rebuilds the tab as a `file` input from the editor's path (`mod.rs:1846-1876`, kind at `:1863`). The prompt asks per closed editor, not per last tab of a document.
- Window close / Quit / leave: `CloseDecision` (`app-gpui/src/app_models/close_guard.rs:118-245`), driven by `app_shell::decide_close` whose `SaveAll` step runs `Workbench::save_all` (`app-gpui/src/workbench/app_shell.rs:583-586`); `close_state.dirty = unsaved_count` (`app_shell.rs:137-151`). `CloseDecision::saved(Err(e))` always reports `e` (`close_guard.rs:236-240` → `Decided { failure: Some }` → `report_save_failure`, `app_shell.rs:597-601`): there is no quiet "cancelled" outcome. `request_leave` (`app_shell.rs:432-447`) serves both Close Project (`request_close_project`, `:451-463`) and opening another project in place (`confirm_leave`, `:945-952`, from `OpenStep::ConfirmLeave`, `:868-871`), so it cannot tell "keep untitled" from "drop untitled" today.
- Opening a project in place: `Workbench::open_project` (`mod.rs:3326-3450`) replaces `self.content` with `Content::Opening` before the backend call (`mod.rs:3340`), which drops every editor; an editor's release closes its document (`view.rs:446-450`), and `Documents::close` removes a document at zero refs (`documents.rs:619-645`), so by the time `Host::open_project` reads `untitled_buffers()` (`project.rs:93`) the buffers are gone. Hypothesis (ordering of entity release versus the backend task not run): the carry would lose them every time. The new content's tabs come only from the stored session (`mod.rs:3389-3393`).
- Session: `editor::session::serialize_session` keeps `file` tabs and tabs whose kind `restorable` says so (`app-gpui/src/editor/session.rs:83-96`); `tab_views::restorable` is false for a kind not in `TAB_KINDS` (`tab_views.rs:229-231`), so an `untitled` tab is left out already. But both writers add a caret entry for every editor's path: `save_session` (`mod.rs:3958-3972`, `files.insert(editor.path()…)` at `:3969`) and `save_light_session` (`app-gpui/src/workbench/light_edit_tabs.rs:232-243`, `:239`).
- Titles: `tab_titles_of` falls back to the text after the last `/` of the path (`mod.rs:1489-1506`), i.e. `untitled:Untitled-1`; `tab_views::icon_of` / `base_title` likewise (`tab_views.rs:234-250`); `Workbench::title` → `window_title` uses `Path::file_name` (`mod.rs:2235-2248`, `:448-469`), i.e. `untitled:Untitled-1`; `light_edit_title` splits on `/` and `\` (`light_edit_tabs.rs:224-228`); `nav_segments` (`app-gpui/src/editor_model/nav_bar.rs:41-75`) would push a folder segment for the path's parent.
- Already right for an untitled path, because native `is_virtual_uri("untitled:Untitled-1")` is true (`app-gpui/src/platform/virtual_docs.rs:32-39`, test `:187`): `basename()` gives `Untitled-1` (`virtual_docs.rs:135-140`, `virtual_name` `:82-85`); status bar read-only item treats it as not on disk (`app-gpui/src/workbench/chrome.rs:254`); the gutter skips VCS (`app-gpui/src/workbench/editor_gutter.rs:224`); context menus drop disk-file rows (`app-gpui/src/workbench/context_menus.rs:241`, `:292`, `:848`); run context (`app-gpui/src/workbench/run_context.rs:131`, `:433`), HTTP client (`app-gpui/src/workbench/http_client.rs:540-543`, `:630`), Annotate (`app-gpui/src/workbench/vcs_text_actions.rs:277`).
- Already right because they filter `kind == "file"`: the tab menu's Copy Path rows (`mod.rs:3606`), Reopen Closed Tab's reload (`mod.rs:3835`, wrong in the other direction, see (d)), the Project view's Open Files and Always Select Opened File (`app-gpui/src/workbench/project_view_options.rs:203`, `:220`), plugin/MCP open editors (`app-gpui/src/workbench/plugin_runtime.rs:1508`, `:1536`), `open_file_paths` (`app-gpui/src/editor_model/controller.rs:49`), detaching a tab to its own window refuses a non-file, non-lifecycle tab (`app-gpui/src/editor_model/detach.rs:74-77`).
- Not filtered: `note_active_tab` marks the active tab's path recent for any kind (`mod.rs:2771-2790`, `ide_host::ops::nav::mark_recent`, `crates/ide-host/src/ops/nav.rs:96-99`). The webview does the same for every active editor path (`src/features/search/common/tracking.ts:160`); Hypothesis: the webview therefore also marks `untitled:` paths recent.
- Entry points that would offer the action: native File menu (`app-gpui/src/workbench/menu_spec.rs:182-212`) has no New submenu and no `file.newUntitled`; LightEdit empty-editor hints lack "New File" (`app-gpui/src/workbench/empty_editor.rs:30-33`); the native Welcome has no "New File" button (no match in `app-gpui/src/workbench/welcome*.rs`).
- Entering LightEdit natively only happens with a file (`open_loose_file`, `tab_views.rs:906-965`, `Host::open_light`); `light_edit_content(editor, …)` builds `Content::Editor` around the first file's editor merged with the stored LightEdit session (`light_edit_tabs.rs:69-116`). Nothing calls `Host::open_light_empty`.
- Save dialog: the native code uses `cx.prompt_for_new_path(&dir, Some(name))` (`app-gpui/src/workbench/app_actions.rs:875`, `database_extras.rs:1004`, `shelf.rs:1235`) and tests answer it with `cx.simulate_new_path_selection(…)` (`app_actions.rs:1830`). Hypothesis: the gpui-kit 0.9.1 API takes no file-type filters.
- Locale keys `app.unsaved.fileBody`, `app.unsaved.saveFile`, `app.action.newFile` and the text "Untitled files were not saved" exist in the shared locale files the native i18n includes (`src/platform/locales/en.json:22`, `:65`, `:68`; `src/platform/locales/text/en.json:2148`; `app-gpui/src/platform/i18n.rs:46-48`).
- Native test harness: `workbench::tests::start` gives the backend a temp `AppDirs` (`mod.rs:4607-4626`) but does not change `HOME`; `Host::open_light_empty` reads `dirs::home_dir()` (`project.rs:726`), and `open_light_in` builds an index, history, LSP manager and plugin diagnostics for that folder (`project.rs:744-780`).

## (c) Design

### Shape: a tab kind of its own, a document like any other

An untitled buffer is a tab with `EditorInput { kind: "untitled", key: uri, path: Some(uri), data: None }` whose editor is an ordinary `Editor` on the backend's untitled document. Why a separate kind over reusing `kind: "file"` with an `untitled:` path: every `kind == "file"` filter listed in (b) (session, Open Files, plugins/MCP, copy-path rows, `open_file_paths`, detach) then excludes it for free and correctly, the webview spec asserts `kind: "untitled"` and the switch to `kind: "file"` on Save As, and the loading path (`open_tab_file`, which must not see it) branches on kind already. The cost is the handful of places that mean "a text tab" and must now accept both kinds; they are listed in (d) and go through one predicate.

New module `app-gpui/src/editor_model/untitled.rs` (pure, no view; registered in `editor_model/mod.rs`):

```rust
//! Untitled buffers: the tab input of a new-file buffer, its display name, which tabs show it,
//! and what each kind of close does with it.

use crate::editor::tabs::{EditorInput, TabsState};

pub const UNTITLED_KIND: &str = "untitled";
pub const UNTITLED_SCHEME: &str = "untitled:";

/// Whether `path` is an untitled buffer's key (`untitled:Untitled-1`).
pub fn is_untitled_uri(path: &str) -> bool { path.starts_with(UNTITLED_SCHEME) }

/// `untitled:Untitled-1` → `Untitled-1`; any other path unchanged.
pub fn untitled_title(path: &str) -> &str { path.strip_prefix(UNTITLED_SCHEME).unwrap_or(path) }

/// The tab input of the buffer `uri`.
pub fn untitled_input(uri: &str) -> EditorInput {
    EditorInput { kind: UNTITLED_KIND.to_owned(), key: uri.to_owned(), path: Some(uri.to_owned()), data: None }
}

/// Tabs shown by the text editor: files and untitled buffers.
pub fn is_text_kind(kind: &str) -> bool { kind == "file" || kind == UNTITLED_KIND }

/// Keys of untitled buffers that `before` shows and `after` does not (their last tab closed).
pub fn closed_untitled_keys(before: &TabsState, after: &TabsState) -> Vec<String>;

/// The untitled tabs of `tabs` in visual order, each key once, with whether it is the active tab
/// of the active pane: what follows the window into another project.
pub fn carried_untitled(tabs: &TabsState) -> Vec<(EditorInput, bool)>;

/// Where Save As opens and the name it suggests: the project root, else the folder of the most
/// recently used file tab of any pane (pane MRU order, active pane first), else `home`; the name
/// is the buffer's title without extension.
pub fn save_as_start(root: Option<&str>, tabs: &TabsState, home: &std::path::Path, key: &str) -> (std::path::PathBuf, String);

/// What a leave does with untitled buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UntitledPolicy {
    /// Another local project opens in this window: they follow it, uncounted and unsaved.
    Keep,
    /// The window closes, quits, or goes to Welcome: they count, and Save must give each a path.
    Settle,
}
```

`TabsState` gets one method (`app-gpui/src/editor/tabs.rs`):

```rust
/// Gives every tab showing `old_key`, in every pane, the input `input` (Save As turned an untitled
/// buffer into a file). Returns whether a tab changed.
pub fn retarget_key(&mut self, old_key: &str, input: EditorInput) -> bool;
```

The closed-tab stack keeps the old entry (Hypothesis: harmless; see Reopen below).

`Editor` gets (`app-gpui/src/editor/view.rs`):

```rust
/// Save As turned this untitled buffer into the file `info` points at: the editor takes its path,
/// language, encoding, line ending and .editorconfig, and is clean. Same document id.
pub fn retarget(&mut self, info: &DocumentInfo, cx: &mut Context<Self>);

/// Whether the document is an untitled buffer (no file behind it yet).
pub fn is_untitled(&self) -> bool { is_untitled_uri(&self.path.to_string_lossy()) }
```

`retarget` sets `path`, `read_only`, `line_ending`, `encoding`, `editorconfig`, `dirty = false`, calls `change_language(info.language.clone(), cx)` only when it differs (the backend's `DocLanguage` event may arrive first or second; `change_language` must be idempotent: Hypothesis, check it), then `read_facts()` and `cx.notify()`. Every editor of the document is retargeted through `DocumentEditors` (`view.rs:413`), not only the one that asked.

### Opening

`Workbench` (new file `app-gpui/src/workbench/untitled_tabs.rs`, an `impl Workbench` block like `light_edit_tabs.rs`):

```rust
/// File › New › Untitled File: a new buffer in a tab of this window. A window that shows nothing
/// (Welcome) becomes a LightEdit window without a file first. A remote window refuses (desktop only).
pub(super) fn new_untitled(&mut self, window: &mut Window, cx: &mut Context<Self>);

/// Gives tab `tab_id` the editor of the untitled buffer `key`, minting nothing: the buffer is
/// re-referenced (`open_untitled(Some(key))`); used by splits, reopened tabs and carried tabs.
fn load_untitled_tab(&mut self, tab_id: String, key: String, focus: bool, window: &mut Window, cx: &mut Context<Self>);
```

`new_untitled` in a `Project` or `Editor` content: `ops::doc::open_untitled(&host, &label, None)` on the backend, then on the UI thread open a tab with `untitled_input(&info.path)` in the active pane (`OpenTabOptions { activate: true, .. }`, not preview) and install its editor from that `DocumentInfo` (no second open). In `Content::Empty`: call `Host::open_light_empty_in(label, dir)` on a blocking task (see the seam below), then build `Content::Editor` with the LightEdit session merged around the new untitled tab (generalise `light_edit_content(editor, …)` to take the tab input: `light_edit_content(input: EditorInput, editor: Entity<Editor>, …)`; the file path case passes `file_input(path)`), then `light_edit_opened(window, cx)`.

The editor-installing half of `load_tab_probed` (`mod.rs:4013-4095`: snapshot the rope, `Editor::with_text`, `install_intelligence`, restored view, focus subscription, `wrap_markdown`, `serve_reveal`, the four `*_opened` watchers) moves into one function both paths call:

```rust
/// Tab `tab_id` got its document: its editor is built and wired, or the tab goes away with the
/// failure reported (a tab without a document would be a dead end).
fn install_tab_document(&mut self, tab_id: &str, opened: Result<DocumentInfo, String>, focus: bool, window: &mut Window, cx: &mut Context<Self>);
```

Why: the alternative (a second copy of that wiring for untitled tabs) would drift the first time someone adds a watcher. `load_tab_probed` keeps the probe/viewer branch and calls `install_tab_document` for the text case.

`load_active_tab` and `reopen_closed_tab` branch: `kind == "file"` → `load_tab` as today; `kind == "untitled"` → `load_untitled_tab`. A reopened untitled tab whose buffer is gone gets a fresh empty buffer under the same name (that is what `open_untitled(Some(key))` does, `documents.rs:545-595`); Hypothesis: the webview behaves the same through `acquire` (`document.ts:832-833`).

Ref counting: every `Editor` owns exactly one backend reference (it closes one on release, `view.rs:446-450`). `open_untitled(None)` gives one; `open_untitled(Some(key))` gives one more (`reopen`, `documents.rs:607-617`). So each editor of an untitled buffer is built from exactly one `open_untitled` call, never from a shared `DocumentInfo` without its own call.

The Welcome seam: `Host::open_light_empty` roots the project at the real home (`project.rs:725-728`). Add to `crates/ide-host/src/project.rs`:

```rust
/// LightEdit without a file, rooted at `dir` (the home folder in the app).
pub fn open_light_empty_in(&self, window: &str, dir: PathBuf) -> IdeResult<Arc<Project>> { self.open_light_in(window, dir, None) }
```

and make `open_light_empty` call it. Natively the folder comes from a global `LightEditHome(PathBuf)` that `Backend::init` sets to `dirs::home_dir()` and `workbench::tests::start` sets to the temp home. Why over calling `open_light_empty` directly: a test of the Welcome path would otherwise build an index and plugin diagnostics over the developer's home folder (`project.rs:744-780`).

### Saving

Save As for one buffer (in `untitled_tabs.rs`):

```rust
/// Asks where to save the untitled buffer of `editor` and saves it there. Resolves to whether it
/// is a file now; `false` = the dialog was cancelled or the save failed (reported as
/// "Cannot save file" with the backend's message). Every tab showing it becomes a file tab and
/// every editor of it takes the new path.
pub(super) fn save_untitled(&mut self, editor: &Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) -> Task<bool>;
```

It runs the save cleanup the editor runs before an explicit save (Hypothesis: native runs the trailing-whitespace / final-newline cleanup inside `Editor` before `ops::doc::save`; find where and call the same step before `save_as`, else the "draft\n" final newline of the spec is missing), then `cx.prompt_for_new_path(&dir, Some(&name))` with `(dir, name) = save_as_start(project_root, tabs, home, key)`, then `ops::doc::save_as(host, label, doc, path)`, then `tabs.retarget_key(key, file_input(path))`, `Editor::retarget` on every editor of the doc, `tabs_changed(false, …)` (session, Open Files, plugin reports and recent pick up the new file tab), and `DocDirty` is already sent by the backend.

Save All and close decisions:

```rust
/// What saving every document ended in.
pub(super) enum SaveAllOutcome {
    Saved,
    /// A Save As dialog was cancelled: an untitled buffer is still unsaved. Nothing is reported.
    Cancelled,
    /// A save failed: its message.
    Failed(String),
}

/// Saves every unsaved document: files at once, then each unsaved untitled buffer through Save As,
/// one dialog at a time. `UntitledPolicy::Keep` leaves the untitled buffers alone.
fn save_all(&self, policy: UntitledPolicy, window: &mut Window, cx: &mut Context<Self>) -> Task<SaveAllOutcome>;

/// Unsaved documents the close question counts: with `Keep`, the untitled buffers are left out.
pub fn unsaved_count(&self, policy: UntitledPolicy, cx: &App) -> usize;
```

`save_documents` (files only) skips editors with `is_untitled()`. The untitled part iterates the distinct dirty untitled documents (one editor each) and awaits `save_untitled` sequentially; any `false` makes the outcome `Cancelled` unless a failure was already recorded. Why sequential: one native save sheet at a time is the platform norm (Hypothesis about gpui's macOS backend allowing only one; see (h)). `save_all_documents` (File › Save All) uses `Settle` semantics but ignores `Cancelled`.

`close_guard.rs`: add `pub fn save_cancelled(&mut self) -> CloseStep { self.decided(false, None) }` beside `saved`. `app_shell::decide_close`'s `SaveAll` step maps `Saved → decision.saved(Ok(()))`, `Failed(e) → decision.saved(Err(e))`, `Cancelled → decision.save_cancelled()`. The decision itself stays pure and testable.

`WindowCloseState` gets the policy: `close_state(&self, policy, cx)` fills `dirty` with `unsaved_count(policy, cx)`; `request_leave(workbench, policy, window, cx)` takes it: `confirm_leave` (another project in place) passes `Keep`, `request_close_project` and the `ToWelcome` route pass `Settle`, window close and Quit pass `Settle`. The policy travels to the `SaveAll` step (store it in the decision's context, or capture it in `decide_close`'s closure).

Autosave: `autosave` skips untitled editors (the webview's `reason === "auto"` no-op). Hypothesis: if native has an idle-save that runs `file.save` like the webview's, it would open Save As dialogs; parity says it should, product sense says it should not; flag it (h).

### Closing a tab

In `tabs_changed` (`mod.rs:1741-1800`), before the per-editor loop, compute `closed_untitled_keys(before, after)`; this needs the state before the change, so `with_tabs` and every other caller that mutates tabs then calls `tabs_changed` must hand the previous `TabsState` (clone before the change; `TabsState` is `Clone`, `editor_model/documents.rs:114`). Alternative considered: decide per editor as today and ask when "no other open tab shows this key" by scanning `after`; that is equivalent and needs no `before`, so prefer it: for a released untitled editor, ask only if `after.open_keys()` does not contain its key. For such an editor, ignore `auto_save`: always prompt with title `app.unsaved.title`, body `app.unsaved.fileBody {name: Untitled-1}`, answers `[app.unsaved.saveFile, app.unsaved.discard, common.cancel]`. Save → `save_untitled`; `true` → `closed_tab_settled`; `false` → `restore_unsaved`. Don't Save → settled (the editor drops, the buffer closes). Cancel → `restore_unsaved`. A clean untitled editor, or one whose key is still shown, releases silently.

`restore_unsaved` builds the input from the editor: `untitled_input(path)` when `editor.is_untitled()`, else `file_input(path)` (fixes `mod.rs:1863`).

### Following the window into another project

`open_project` (`mod.rs:3326`): before `self.content = Content::Opening(…)`, take `carried_untitled(tabs)` and the `Entity<Editor>` of one editor per carried key, and move them into the spawned task so the documents stay referenced until `ops::project::open` returns (the backend copies them at `project.rs:93`). After the project opened, open the carried inputs in the first pane of the new `TabsState` (after the restored session's tabs, activate the one that was active, as `controller.ts:407-422`), then drop the old editors (their release closes ids of the old registry, which `Documents::close` ignores: `documents.rs:623`, unknown id returns). The new tabs load through `load_untitled_tab`, whose `open_untitled(Some(key))` re-references the carried buffer. The carried buffer was created with one orphan reference by `restore_untitled_buffer` (`documents.rs:1131-1132`); `load_untitled_tab` for a carried tab closes one reference after its editor is installed, so the editor's reference is the only one and closing the tab really closes the buffer. Hypothesis: the webview leaks that reference (it calls `openUntitled(path)` on top of the restore and closes once); not verified.

Leaving for Welcome, Close Project, closing the window or quitting: `Settle` semantics above; after Don't Save the buffers close with the project.

### Titles and the rest

- `tab_titles_of`: name = `untitled_title(path)` for `kind == "untitled"`; disambiguation hints skip untitled paths.
- `tab_views::icon_of` / `base_title`: `untitled_title` before taking the file name.
- `Workbench::title` and `light_edit_title`: pass `untitled_title(path)`.
- `nav_segments`: an untitled path yields one `NavKind::File` item labelled `Untitled-1` (statusItems spec `src/editor/core/view/statusItems.test.tsx:271-273`).
- `save_session` / `save_light_session`: skip editors with `is_untitled()` when filling `files`.
- `note_active_tab`: skip `mark_recent` for untitled keys (Hypothesis: a parity deviation; see (h)).
- Action: `registry.handle("file.newUntitled", …)` in `app-gpui/src/workbench/windows.rs` beside `file.openFile` (`:221`), refusing in a remote window the way other desktop-only actions do (find the native `desktopOnly` equivalent; `app-gpui/src/platform_more/remote.rs:127` is where remote windows gate commands).
- Menu: `act("file.newUntitled", "Untitled File")` after `file.newScratchFile` in `menu_spec.rs:188` (the native File menu has no New submenu; making one belongs to `F-WB-MENUS-TITLE`).
- LightEdit empty hints: `Hint { label: "New File", action: Some("file.newUntitled"), fallback: None }` first in `empty_editor.rs:30-33` (matches `EditorArea.tsx:20-24`).
- Welcome "New File" buttons: left to `F-SMALL-FEATURES-A` (its welcome rows), which depends on this slice; it only needs the action.

## (d) Existing native files that change

- `app-gpui/src/editor_model/mod.rs`: `pub mod untitled;`.
- `app-gpui/src/editor_model/untitled.rs` (new): the module above with its unit tests.
- `app-gpui/src/editor/tabs.rs`: `TabsState::retarget_key`.
- `app-gpui/src/editor/view.rs`: `Editor::retarget`, `Editor::is_untitled`.
- `app-gpui/src/editor_model/nav_bar.rs:41`: untitled path → one File segment.
- `app-gpui/src/workbench/mod.rs`: `mod untitled_tabs;`; `tab_titles_of` (1489-1506); `restore_unsaved` input kind (1863); `tabs_changed` untitled close question (1757-1800); `unsaved_count` with policy (1909); `autosave` skips untitled (1921-1942); `save_documents` skips untitled (1946-1964); `save_all` with policy and `SaveAllOutcome` (1983-1999), `save_all_documents` (1968); `title` (2235-2248); `note_active_tab` (2771-2790); `open_project` carry (3326-3450); `reopen_closed_tab` (3829-3841); `load_active_tab` (3845-3876); `save_session` (3958-3972); `load_tab_probed` split into `install_tab_document` (3988-4100); `tests::start` sets `LightEditHome` (4607-4626).
- `app-gpui/src/workbench/untitled_tabs.rs` (new): `new_untitled`, `load_untitled_tab`, `save_untitled`, the untitled half of `save_all`.
- `app-gpui/src/workbench/light_edit_tabs.rs`: `light_edit_content` takes the first tab's input (69-116); `light_edit_title` (224-228); `save_light_session` skips untitled (232-243).
- `app-gpui/src/workbench/tab_views.rs`: `icon_of` / `base_title` (234-250).
- `app-gpui/src/workbench/app_shell.rs`: `close_state(policy)` (137-151); `request_leave(policy)` (432-447) and its callers `request_close_project` (451-463), `confirm_leave` (945-952), the `ToWelcome` route (414-421); `decide_close` `SaveAll` step (583-586).
- `app-gpui/src/app_models/close_guard.rs`: `CloseDecision::save_cancelled`.
- `app-gpui/src/workbench/windows.rs`: the `file.newUntitled` handler (near 221).
- `app-gpui/src/workbench/menu_spec.rs:188`: the menu entry.
- `app-gpui/src/workbench/empty_editor.rs:30-33`: the LightEdit hint.
- `app-gpui/src/backend.rs`: the `LightEditHome` global set in `Backend::init` (185).
- `crates/ide-host/src/project.rs:725-728`: `open_light_empty_in`.
- `docs/gpui-port/INVENTORY.md` / `parity.json`: rows `act:file.newUntitled` (INVENTORY.md:471), `ipc:doc_open_untitled` (:2421), `ipc:light_edit_open_untitled` (:2524), `unit:src/editor/core/untitled.test.ts`, and the keymap rows' case counts (:1110-1111), through the project's own tooling.

## (e) Parts, in order (each lands alone, the app works after each)

1. Part 1, open and show (Hypothesis: about 600 lines with tests). `editor_model/untitled.rs` (input, title, `is_text_kind`, `closed_untitled_keys`, `carried_untitled`, `save_as_start`, `UntitledPolicy`), `TabsState::retarget_key`, `install_tab_document` extracted from `load_tab_probed`, `untitled_tabs.rs` with `new_untitled` (project and LightEdit windows only) and `load_untitled_tab`, the `load_active_tab` / `reopen_closed_tab` branches, titles/icon/window title/nav bar, session writers skip untitled, `note_active_tab` skip, the action, menu entry and LightEdit hint. Data safety in this part alone: autosave and close must already not lose an untitled buffer, so include here the two guards: `autosave` and `save_documents` skip untitled, and `tabs_changed` always asks for the last tab of a dirty untitled buffer. Save As does not exist yet in Part 1, so its close question offers Don't Save and Cancel only; Part 2 adds Save. Window close and Quit in Part 1 still count the buffer but their Save would leave it unsaved, so Part 1 also makes `save_all` return a failure ("Untitled files were not saved") when a dirty untitled buffer remains, which keeps the window open. Hypothesis: a two-button question for one part is acceptable since it never loses data.
2. Part 2, Save As (about 650). `Editor::retarget`, `save_untitled`, the Save button of the tab-close question, `SaveAllOutcome`, `save_all(policy)`, `unsaved_count(policy)`, `CloseDecision::save_cancelled`, `decide_close`'s `SaveAll` step, `request_leave(policy)` and its callers, `restore_unsaved` input kind. After this part window close / Quit / Close Project are safe with untitled buffers.
3. Part 3, follow the window and Welcome (about 550). The `open_project` carry with the reference hand-over, `open_light_empty_in` + `LightEditHome`, `new_untitled` from `Content::Empty`, `light_edit_content(input, …)`.

Hypothesis: total about 1,800 lines, twice the plan's hand estimate of 900 (WORKPLAN.md:510-527); the difference is the close guard and the project-switch carry, which the estimate did not count. Order: 1 → 2 → 3; 3 does not depend on 2 for compiling, but a carried buffer must be closable with Save, so keep the order.

## (f) Tests per part

All workbench tests start with `let _home = workbench::tests::start(cx);` and a `tempfile` project folder; the save dialog is answered with `cx.simulate_new_path_selection(|_| Some(path_in_tempdir))`, prompts with `cx.simulate_prompt_answer(label)` (as `mod.rs:5070-5091`).

Part 1

- `editor_model::untitled` unit tests: title of `untitled:Untitled-1`; `untitled_input` shape; `is_text_kind`; `closed_untitled_keys` asks only when the last split goes; `carried_untitled` order and active flag; `save_as_start` (root, LightEdit MRU file folder, home fallback, name without extension).
- `TabsState::retarget_key`: two panes showing the key both change; other tabs untouched.
- Spec `untitled.test.ts` "opens editable, pathless buffers with incrementing names": run `file.newUntitled` twice in a project window; tab inputs `{kind: "untitled", key/path: untitled:Untitled-1}` then `…-2`; tab title `Untitled-1`; editor language `plaintext`, not read-only, not dirty; type → dirty.
- Spec "reuses the buffer for a re-opened untitled name and shares it across tabs": split the untitled tab; both editors have the same `doc()`; typing in one shows in the other.
- Session (part of spec "autosave never writes…"): with an untitled tab open and edited, `save_session` then `load_session` holds no `untitled:` anywhere (tabs nor `files`).
- Autosave: with `files.autoSave` on, switching tabs and losing window focus leaves the untitled buffer dirty and writes no file in the project folder.
- Close (Part 1 form of spec "closes an untitled buffer without saving when the user declines"): Cancel keeps the tab and text; Don't Save closes it, no file written; closing one of two split tabs asks nothing.
- Breadcrumbs spec `statusItems.test.tsx:271`: `nav_segments(Some(("/p","p")), "untitled:Untitled-1", &[])` is one File item `Untitled-1`.
- Window title: a LightEdit window on an untitled tab is titled `Untitled-1`.
- Keymap: with the vscode-macos preset, `Meta+N` runs `file.newUntitled` (dispatch test in the existing keymap e2e style, `app-gpui/src/workbench/tests/proofs18_e2e_keymap.rs`).

Part 2

- Spec "autosave never writes an untitled buffer; explicit save asks for a path and retargets tab and model": `file.save` with the path picker answering `<tmp>/new/dir/hello.rs`: the tab becomes `{kind: "file", key/path: <tmp>/new/dir/hello.rs}`, `editor.path()` is it, the editor is clean and its language is `rust`, the file holds `draft\n` (final newline from the save cleanup), the next session holds the path and no `untitled:`.
- Spec "offers the registered languages as save-as types": only its `defaultPath` half can be proved (the picker is opened at the project root with the name `Untitled-1`, read through the `simulate_new_path_selection` callback's argument, Hypothesis: the callback receives the directory); the filter half is open (h).
- Spec "keeps the buffer open and dirty when the save dialog is cancelled": picker answers `None`: tab still untitled, dirty, no file written.
- Spec "asks about a dirty untitled buffer when its last tab closes": Cancel keeps it; Save with the picker answering `<tmp>/kept.txt` closes it and the file holds `draft\n`.
- Save As refused: an untitled buffer saved onto a path open in another tab reports "Cannot save file" and stays untitled and dirty.
- Spec `windowClose.test.ts:118` "stays open when an untitled buffer's Save As is cancelled": two windows, close one with a dirty untitled buffer, answer Save All, picker `None`: the window stays, no notification.
- Spec `closeGuard.test.ts:285` "counts untitled buffers only when leaving for Welcome, and stays when one isn't saved": Close Project with one dirty untitled buffer asks "1 file has unsaved changes…", Save + picker `None` keeps the project open quietly, Save + a path closes it.
- `CloseDecision` unit test: `save_cancelled` decides `close: false` with no failure.
- Quit with a dirty untitled buffer and Don't Save: the window closes (the buffer is dropped, as in the webview).

Part 3

- Opening another project in place with a dirty and a clean untitled buffer: no unsaved question for them; after the open both tabs are there (dirty one still dirty, text kept, the active one active); closing the dirty one's tab still asks; after Don't Save, `Documents::untitled_buffers()` of the new project is empty (the reference hand-over).
- From Welcome: `file.newUntitled` turns the window into LightEdit (rooted at the test's temp home through `LightEditHome`) with one untitled tab, focused; the stored LightEdit session's tabs come back around it.
- `crates/ide-host/tests/light_edit.rs`: `open_light_empty_in` roots the project at the given folder.

## (g) What tests and the real window must never do

- HOME: no test calls `Host::open_light_empty` (it roots at the real home); tests use `open_light_empty_in` with a temp folder through `LightEditHome`. No save dialog in a test or in a real-window check points into the user's home: every `simulate_new_path_selection` answer is inside a `tempfile` folder, and a real-window check saves into a temp folder typed into the dialog.
- No test leaves a real native save sheet open waiting for input (always `simulate_new_path_selection` before the action that opens it).
- The real window: create untitled buffers only in a project or LightEdit window on a temp folder; never answer Save into a real project of the user's; never quit with Don't Save while a real unsaved file of the user's is open.

## (h) Open questions only a compiler or the running app can settle

- Does gpui-kit 0.9.1's `prompt_for_new_path` take file-type filters (the webview's "Text files / languages / All files" dropdown)? Settle: read `prompt_for_new_path` and `PathPromptOptions` in the gpui-kit 0.9.1 source the build fetched, or compile a call with a third argument. If it does not, the spec case "offers the registered languages as save-as types" stays partial and the row says so.
- What does `simulate_new_path_selection`'s callback receive (the directory, the suggested name)? Settle: compile a test that asserts on it.
- Can two `prompt_for_new_path` sheets be open at once on macOS? Settle: run Save All with two dirty untitled buffers in the real window; the design is sequential either way.
- Is `Editor::change_language` idempotent, and does the `DocLanguage` event emitted inside `save_as` (`documents.rs:895-897`) reach the editor before or after `retarget`? Settle: a test that saves an untitled buffer as `.rs` and asserts the highlights and language once.
- Where does native run the save cleanup (final newline, trailing whitespace) before `ops::doc::save`, and can `save_untitled` call the same step? Settle: read `Editor` / `editor_model::document` callers at the writer's HEAD; the spec expects `draft\n`.
- Does `EditorIntelligence` read the editor's path per request or once at install? After Save As the LSP must see the new path. Settle: a test that saves an untitled buffer as `.rs` in a project with a Rust server fixture and asks for a hover; or read `install_intelligence`.
- Is the entity release of the old editors in `open_project` really before the backend's `untitled_buffers()` read (the loss claimed in (b))? Settle: the Part 3 test without the carry fails if it is.
- Recent files: the webview marks every active editor path recent, untitled included (Hypothesis, `tracking.ts:160`). Native skips it in this design; confirm with the parity owner or run the webview and look at ⌘E after creating an untitled buffer.
- Idle save: does native have the webview's idle `file.save` timer (`appearanceRuntime.ts:252-260`)? If it does, it would open Save As dialogs; decide with the parity owner. Settle: `git grep -n idle HEAD -- app-gpui/src/workbench` at the writer's HEAD and a run.
- Breakpoints and Markdown layouts on an untitled buffer, and Markdown layouts after Save As to `.md` (`wrap_markdown` runs only at load): Settle in the running app.
