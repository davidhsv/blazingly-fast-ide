# Integrating the Project view's options, panes and file actions, and the IDE zoom

New files, not compiled yet:

- `app-gpui/src/project_view/tree_clicks.rs`: what a row click and a selection move do, by the view options; the `OpenRequest` event of `ProjectTree`.
- `app-gpui/src/project_view/pane.rs`: `TreePane`, the pane the tree shows ("Project", "Project Files", "Open Files", "All Changed Files") and its model.
- `app-gpui/src/theme/zoom.rs`: `apply_zoom`, which scales the theme's lengths.
- `app-gpui/src/workbench/project_view_options.rs`: actions, handlers, end-to-end tests.
- `app-gpui/src/workbench/zoom_actions.rs`: `view.zoomIn`, `view.zoomOut`, `view.resetZoom`, `view.quickSwitchScheme`, end-to-end tests.

Every anchor below is quoted from the file as it is now; no line numbers. Sections 1 to 6 are all required: without section 3 the tree ignores the options and the panes, without 4e and 5 the Open Files and All Changed Files panes stay empty, and without 6a `header_menu_rows` is dead code (`-D warnings`). Run `cargo fmt` after pasting (a few test lines run past 120 columns).

## 1. Module lines

`app-gpui/src/project_view/mod.rs`, in alphabetical order (after `pub mod operations;`, and after `pub mod store;`):

```rust
pub mod pane;
```

```rust
pub mod tree_clicks;
```

`app-gpui/src/theme/mod.rs`, after `pub mod tokens;`:

```rust
pub mod zoom;
```

`app-gpui/src/workbench/mod.rs`, after `mod project_view_actions;`:

```rust
mod project_view_options;
```

and after `mod vcs_operations;`:

```rust
mod zoom_actions;
```

## 2. `app-gpui/src/theme/appearance.rs`: the zoom is part of the theme

In `pub fn apply(cx: &mut App)`, replace

```rust
        for (name, value) in &applied.overrides {
            theme.set(name, value);
        }
        cx.set_global(ActiveTheme { theme });
```

with

```rust
        for (name, value) in &applied.overrides {
            theme.set(name, value);
        }
        // The IDE zoom scales every length of the theme: text sizes, row heights, paddings.
        super::zoom::apply_zoom(&mut theme, options.zoom);
        cx.set_global(ActiveTheme { theme });
```

`options` is the `AppearanceSettings` that `apply` already reads (`let options = settings::global(cx).appearance;`). `apply` already runs on every `appearance.*` settings change (`init`'s listener), so a zoom written from Settings › Appearance applies too.

## 3. `app-gpui/src/project_tree.rs`: the tree honours the options and shows a pane

### 3a. Imports

Replace

```rust
    FsChange, Listings, ProjectNode, SortKey, TreeModel, TreeOptions, build_model, drop_listings, is_dir_entry,
```

with

```rust
    FsChange, Listings, ProjectNode, SortKey, TreeModel, TreeOptions, drop_listings, is_dir_entry,
```

(`build_model` is only used in `rebuild`, which 3d replaces.) After `use crate::project_view::store::ProjectViewState;` add:

```rust
use crate::project_view::pane::TreePane;
use crate::project_view::tree_clicks::{OpenRequest, RowClick, row_click, selection_open};
```

### 3b. The field

In `pub struct ProjectTree`, after

```rust
    /// A path to select and scroll to once its row exists.
    reveal: Option<String>,
```

add

```rust
    /// The pane shown, and the files the filtered panes list.
    pane: TreePane,
```

### 3c. Its initial value

In `ProjectTree::new`, after `let options = storage.map(ViewOptions::stored).unwrap_or_default();` add

```rust
        let pane = TreePane::stored(storage);
```

and in `let mut tree = Self { … }`, after `reveal: None,` add `pane,`.

### 3d. The model of the pane

In `fn rebuild`, replace

```rust
        self.model =
            build_model(&self.root, &self.name, &self.listings, &self.options.tree_options(), &self.ignored, None);
```

with

```rust
        self.model = self.pane.model(&self.root, &self.name, &self.listings, &self.options, &self.ignored);
        // The folders of an "Open Files" or "All Changed Files" pane open as they appear.
        self.pane.auto_expand(&self.model, &mut self.expanded);
```

### 3e. The operations' state knows the pane

In `pub fn mirror_into`, after `state.options = self.options.clone();` add

```rust
        state.mode = self.pane.mode();
        state.set_open_files(self.pane.open_files());
        state.set_changed_files(self.pane.changed_files());
```

### 3f. The tree takes over a pane switched by an operation

In `pub fn follow_state`, replace

```rust
        if state.expanded != self.expanded {
            self.expanded.clone_from(&state.expanded);
            self.expansion_changed(cx);
        }
```

with

```rust
        // The pane first: the expansion that comes with it is the new pane's.
        let switched = self.pane.set_mode(state.mode);
        if state.expanded != self.expanded {
            self.expanded.clone_from(&state.expanded);
            self.expansion_changed(cx);
        }
        if switched {
            self.rebuild(cx);
        }
```

The order matters: `rebuild` auto-expands the folders of a filtered pane into `self.expanded`. Copying `state.expanded` after it would close them again, and they would not reopen.

### 3g. Only the "Project" pane's expansion is remembered

In `fn write_expanded(&self, cx: &mut App)`, replace

```rust
        if !cx.has_global::<UiStorage>() {
            return;
        }
        let keys: Vec<&String> = self.expanded.iter().take(MAX_PERSISTED).collect();
```

with

```rust
        if !cx.has_global::<UiStorage>() || !self.pane.keeps_stored_expansion() {
            return;
        }
        let keys: Vec<&String> = self.expanded.iter().take(MAX_PERSISTED).collect();
```

### 3h. Arrow keys: Autoscroll to Source

In `fn key_down`, replace

```rust
        let rows = self.rows();
        let current = self.selected.as_ref().and_then(|selected| rows.iter().position(|row| &row.node.key == selected));
```

with

```rust
        let rows = self.rows();
        let before = self.selected.clone();
        let current = self.selected.as_ref().and_then(|selected| rows.iter().position(|row| &row.node.key == selected));
```

and replace

```rust
        if let Some(index) = self.selected.as_ref().and_then(|key| rows.iter().position(|row| &row.node.key == key)) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Top);
        }
```

with

```rust
        if self.selected != before {
            self.selection_changed(cx);
        }
        if let Some(index) = self.selected.as_ref().and_then(|key| rows.iter().position(|row| &row.node.key == key)) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Top);
        }
```

### 3i. Row clicks

In `impl Render for ProjectTree`, replace

```rust
                            .on_click(cx.listener(move |tree, event: &ClickEvent, window, cx| {
                                window.focus(&tree.focus, cx);
                                tree.select(&clicked, cx);
                                if event.click_count() >= 2 {
                                    tree.activate(&clicked, cx);
                                }
                            }))
```

with

```rust
                            .on_click(cx.listener(move |tree, event: &ClickEvent, window, cx| {
                                window.focus(&tree.focus, cx);
                                let plain = !event.modifiers().modified();
                                tree.click_row(&clicked, event.click_count(), plain, cx);
                            }))
```

### 3j. The new methods

Directly before `impl Focusable for ProjectTree {` add:

```rust
impl ProjectTree {
    /// The pane shown.
    pub fn pane(&self) -> &TreePane {
        &self.pane
    }

    /// The files open in editor tabs, which "Open Files" lists.
    pub fn set_open_files(&mut self, paths: &[String], cx: &mut Context<Self>) {
        if self.pane.set_open_files(paths) {
            self.rebuild(cx);
        }
    }

    /// The files with local changes, which "All Changed Files" lists.
    pub fn set_changed_files(&mut self, paths: &[String], cx: &mut Context<Self>) {
        if self.pane.set_changed_files(paths) {
            self.rebuild(cx);
        }
    }

    /// The `count`th click of a burst on the row of `key`; `plain` when no modifier is held. The
    /// row is selected, then the view options decide what else happens.
    fn click_row(&mut self, key: &str, count: usize, plain: bool, cx: &mut Context<Self>) {
        let moved = self.selected.as_deref() != Some(key);
        self.select(key, cx);
        let Some(node) = self.model.by_key.get(key).cloned() else { return };
        match row_click(&self.options, &node, count, plain) {
            RowClick::Select => {}
            RowClick::Toggle => self.toggle(key, cx),
            RowClick::Open { preview: false, focus: true } => self.activate(key, cx),
            RowClick::Open { preview, focus } => {
                cx.emit(OpenRequest { path: PathBuf::from(&node.path), preview, focus });
            }
        }
        if moved && plain {
            self.selection_changed(cx);
        }
    }

    /// The selection moved: with Autoscroll to Source its file opens, the keyboard staying here.
    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        let node = self.selected.as_ref().and_then(|key| self.model.by_key.get(key));
        if let Some(request) = node.and_then(|node| selection_open(&self.options, node)) {
            cx.emit(request);
        }
    }
}
```

The `EventEmitter<OpenRequest>` impl of `ProjectTree` is in `tree_clicks.rs`. A double click on a file still goes through `activate`, so it emits `OpenFile` as before, and the workbench's existing subscription opens it.

### 3k. Files of the filtered panes open

The rows of "Open Files" and "All Changed Files" come from `build_paths_model`. That builds them through `path_node`, which leaves `entry: None`, so `activate` must not require a listing entry. In `pub fn activate`, replace

```rust
        } else if node.entry.is_some() {
            cx.emit(OpenFile(PathBuf::from(&node.path)));
        }
```

with

```rust
        } else if node.placeholder.is_none() && node.member.is_none() && !node.group {
            cx.emit(OpenFile(PathBuf::from(&node.path)));
        }
```

## 4. `app-gpui/src/workbench/mod.rs`

### 4a. The field

In `pub struct Workbench`, after

```rust
    /// The Project view's file operations.
    files: project_files::ProjectFiles,
```

add

```rust
    /// What the Project view's behaviour options need of the window.
    tree_options: project_view_options::TreeOptionsSlot,
```

### 4b. Its initial value

In `Workbench::new`, after `files: Default::default(),` add

```rust
            tree_options: Default::default(),
```

### 4c. The actions

In `pub fn register_actions(cx: &mut App)`, after `project_view_actions::register_actions(registry);` add

```rust
    project_view_options::register_actions(registry);
    zoom_actions::register_actions(registry);
```

`project_view_options` registers `project.option.{autoscrollFromSource,autoscrollToSource,singleClickOpen,singleClickExpand,previewTab}` with `handle_toggle`, and `project.viewMode.{project,projectFiles,openFiles,changedFiles}`, `project.findUsages`, `project.reformatCode`, `project.optimizeImports` and `project.reloadFromDisk` with `handle`. Every id is in `assets/gen/actions.json` (`grep -c` gives 1 for each), and none is registered anywhere else (`grep -rn` over `app-gpui/src` finds no `registry.handle` for them). `project_view_actions.rs` registers only the four tree toggles (`TREE_TOGGLES`) and the sort orders, so the toggles do not collide.

### 4d. The tree's open requests

In `Workbench::open_project`, in the `Ok((root, name, color_name)) => {` arm, replace

```rust
                        let menus = cx.subscribe_in(&tree, window, Self::tree_context_menu);
```

with

```rust
                        let menus = cx.subscribe_in(&tree, window, Self::tree_context_menu);
                        workbench.watch_tree_requests(&tree, window, cx);
```

### 4e. The editor tabs feed the tree

In `fn tabs_changed`, replace

```rust
        self.note_active_tab(cx);
```

with

```rust
        self.note_active_tab(cx);
        self.project_view_tabs_changed(cx);
```

(`self.note_active_tab(cx);` occurs once in `mod.rs`.)

## 5. `app-gpui/src/workbench/branches.rs`: the local changes feed the tree

In `fn vcs_state_changed`, replace

```rust
        self.github_vcs_changed(cx);
        cx.notify();
```

with

```rust
        self.github_vcs_changed(cx);
        self.feed_changed_files(cx);
        cx.notify();
```

## 6. Menu entries

### 6a. The Project tool window's Options menu (`app-gpui/src/workbench/context_menus.rs`)

In `pub(super) fn show_tool_window_options`, replace

```rust
        let items = tool_window_options(layout, id, keymap_shortcut("toolwindow.hideActive", cx));
        self.show_tool_window_menu(id, &items, position, window, cx);
```

with

```rust
        let mut items = tool_window_options(layout, id, keymap_shortcut("toolwindow.hideActive", cx));
        // The Project view's own options come first in its Options menu.
        if id == super::PROJECT_TOOL_WINDOW {
            let mut rows = super::project_view_options::header_menu_rows();
            rows.push(MenuItem::Separator);
            rows.append(&mut items);
            items = rows;
        }
        self.show_tool_window_menu(id, &items, position, window, cx);
```

The rows match the "⋮" menu of `HeaderActions.tsx`:

- "Appearance": Show Hidden Files, Show Excluded Files, Compact Middle Packages, Sort Folders First.
- A separator, then the three sorts.
- Another separator, then "Behavior": the five behaviour options.

Each row is the action itself. The popup checks a row while the action's `toggled` reads on (`popup_menu.rs`: `checked: action.is_toggled(window, cx) == Some(true)`). All those `toggled` closures read only the `UiStorage`, so they are safe to run while the menu is built inside the workbench's update. Show Members is left out because it is not registered (section 8). The menu is opened by the header's `more:project` button (`show_tool_window_options`), and the test `the_options_menu_lists_the_view_options_and_they_survive_reopening_the_project` clicks it.

### 6b. The rest

- The View menu already has `view.zoomIn`, `view.zoomOut`, `view.resetZoom` and `view.quickSwitchScheme` (`menu_spec.rs`: `act("view.zoomIn", "Zoom In IDE")`, `act("view.quickSwitchScheme", "Quick Switch Scheme…")`). Those rows are enabled now.
- **No "Project ▾" pane switcher.** The header draws the title as plain text: `.child(SharedString::from(declared.title.clone()))` in `mod.rs`'s `tool_window` closure. The four panes are therefore reachable from Find Action and the keymap only.
- **No Select Opened File button to hide.** The header has only the Options (`more:{id}`) and Hide (`hide:{id}`) buttons.

## 7. IPC commands the actions stand for

- `project.findUsages`: `doc_open`, `doc_outline`, `lsp_references`, `doc_close`, through the existing `Request::FileUsages` flow of `project_files.rs`.
- `project.reformatCode`: `code.reformatFile`, whose `format_document` sends LSP `textDocument/formatting`.
- `project.optimizeImports`: `code.optimizeImportsFile`, whose `run_kind` sends a `source.organizeImports` code action.
- `project.reloadFromDisk`: `fs_read_dir` (forced, for the loaded folders below the targets), then `file.reloadFromDisk` (`doc_reload`).
- Zoom: `settings_set("global", "appearance.zoom", …)`.
- Quick Switch: `settings_set("global", "appearance.theme" | "editor.colorScheme" | "keymap", …)`.
- Options and panes: UI storage only (`openidea.projectTree.options`, `openidea.projectTree.mode`).

## 8. Not built, and why

- **`project.option.showMembers`, not registered.** `ProjectTree` draws a chevron only for `node.dir` (`render`: `let chevron = if node.dir {`). `rows()` walks `model.children` only, and nothing in the tree reads outlines. The model has `member_nodes` and `ProjectViewState::outlines`, but the view would need outline loading, file chevrons and member navigation.
- **`project.renameInline`, not registered.** The tree has no inline editor: `project_tree.rs` has no text field and does not read `Editing`. New File goes through the New popup instead.
- **`project.viewMode.unity` and `project.viewMode.scratches`, not registered.**
  - Scratches: nothing outside `project_view/` registers a `ProjectViewRoot` with `scratches: true`. The search was `grep -rn "register_root\|scratches: true" app-gpui/src`, excluding `project_view/`. So the pane would be empty.
  - Unity: this is unbuilt wiring, not a missing feature. `ops::project::open` detects Unity projects and `ProjectViewState` has the pane, but `ProjectTree::new` is given no `UnityProjectInfo` and `ViewOptions::tree_options` sets `unity_root: None`. Building it needs the open result's `unity` carried to the tree, and `TreePane::model` building with `ProjectViewState::tree_options`' Unity options.
  - `TreePane::stored` shows either pane, when stored, as "Project".
- **Theme preview on hover in Quick Switch.** `PopupMenu` reports no active-row event. Quick Switch therefore uses `quick_switch_items(.., preview = false)`, so choosing a theme selects it at once, with no 500 ms preview.
- **Checked rows.** `vcs_ops::menu::MenuEntry` has no checked flag, so a check icon stands in for it.
- **Zoom covers the theme's lengths only.** Sizes written as literals in views (`px(16.0)` icons, paddings) do not scale. GPUI has no window-wide content scale: `rem_size` affects only `rems`, which this crate does not use.
- **Viewers from the preview tab.** A preview or keyboard-less open (`open_text_tab_from_tree`) always opens a text tab. The probe still turns binary content into the hex view, but an image opens in the image viewer only from a double click or Enter.

## 9. Assumptions to check first

1. `tabs_changed`, `load_tab_probed`, `Content`'s fields, `tabs`, `editors` and `active_pane` are reachable from the child module `project_view_options`, as private items of `workbench`.
2. `.git` is in the default `files.ignoredNames`, so the git test's rows start with `src`.
3. Closing and reopening the project in the same window (`reopen`) works right after `close_project_detached`. If it races, open the project in a second window `w2` instead: the options and the pane live in the shared `UiStorage`.
4. In Light mode, a `.txt` file's usage search ends `Unsupported`, which raises "Cannot find usages of 'notes.txt'".
5. `theme::appearance::apply` rebuilds the theme whenever `apply_theme` returns `Some`, which is assumed to be always with the default settings.
6. The app calls `theme::appearance::init` at start-up (not read). The tests call it themselves.
7. `platform/keymap.rs` pairs `file.reloadFromDisk` with `project.reloadFromDisk` (`("file.reloadFromDisk", "project.reloadFromDisk")`). I have not read what that pair does now that the second id is registered; check that ⌥⌘Y in the tree runs `project.reloadFromDisk`.
8. The Options menu selects its first enabled row ("Show Hidden Files") when it opens, as the Select In chooser does with its header.
9. A preview open honours `editor.tabs.previewTab`, as the editor's `guard.rs` does (`opts.preview && prefs.preview_tab`, default `true`): with it off, the tree opens a regular tab instead.

## 10. Tests most likely to fail on the first run

- `reformat_and_optimize_imports_in_the_tree_change_the_selected_file`: the fake server must start for a file that `code.reformatFile` opens. That file is not open beforehand, unlike in `editor_language.rs`.
- `switches_panes_and_remembers_the_choice`: depends on assumptions 2 and 3, and on the VCS read reaching `feed_changed_files` after the tree exists.
- `expands_and_collapses_folders_on_single_click_only_when_enabled`: after the reopen it expects `src` still open. That needs the expansion written on the tree's release.
- `the_zoom_actions_step_the_ide_zoom_and_the_window_draws_at_it`: the row height compared at ±0.5 px, rounded by layout.
