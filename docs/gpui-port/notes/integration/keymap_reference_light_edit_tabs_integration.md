# Integrating Keymap Reference and tabs in a single-file window

New files, not compiled yet:

- `app-gpui/src/app_views/keymap_reference_dialog.rs` (the dialog view and its tests)
- `app-gpui/src/workbench/keymap_reference.rs` (the action, the handler and end-to-end tests)
- `app-gpui/src/workbench/light_edit_tabs.rs` (the LightEdit tab model on top of the project tab machinery, with end-to-end tests)

Every anchor below is quoted from the file as it is now. Line numbers are left out because `mod.rs` moves, and another agent is editing compiled files at the same time. Part A stands alone. Part B is one unit: `Content::Editor` changes shape, so every hunk in B is needed for the crate to compile.

Lines inside `let … else` guards that come out longer than 120 columns are written here already broken. Elsewhere, keep the formatting the compiler accepts.

---

## Part A: Keymap Reference (`help.keymapReference`)

### A1. `app-gpui/src/app_views/mod.rs`

After `pub mod invalidate_caches_dialog;` add:

```rust
pub mod keymap_reference_dialog;
```

Also update the module doc: "About, New Project, Invalidate Caches" becomes "About, Keymap Reference, New Project, Invalidate Caches".

### A2. `app-gpui/src/workbench/mod.rs`: the module

Replace

```rust
mod github;
mod log_actions;
```

with

```rust
mod github;
mod keymap_reference;
mod light_edit_tabs;
mod log_actions;
```

`light_edit_tabs` is Part B. Leave it out if only Part A goes in.

### A3. `app-gpui/src/workbench/mod.rs`: registration

After `    app_actions::register_actions(registry);` add:

```rust
    keymap_reference::register_actions(registry);
```

### A4. Declare the waiting test

In `mod tests` of `workbench/mod.rs`, after `    mod proofs13_shell;` add `    mod proofs13_shell_pending;`. You may rename the file to drop `_pending`. Its test passes unchanged.

### What A wires

- **Action.** `help.keymapReference` has no `when`, so it is available in a project window, a single-file window and the Welcome screen (whose Help/Manage menu already lists it in `welcome_model::manage_menu`). The menu entry is already in `menu_spec.rs` (`act("help.keymapReference", "Keymap Reference")`).
- **About link.** About's "Keymap Reference" link now appears, because `show_about` offers it once the action is registered.
- **Dialog slot.** The dialog is shown in the window's dialog slot (`open_vcs_dialog`, drawn through `workbench::dialog_frame` by `vcs_dialog_layer`). Escape, from the filter field or from the dialog, closes it, and `close_vcs_dialog` gives the keyboard back.
- **No IPC.** The webview reads the action registry and the keymap in the UI, and so does this.

### Differences from the webview, on purpose

- **Position.** The webview's `Dialog position="top"` is a light popup near the top that closes when the window deactivates. This one is centred like every other dialog in the slot, and stays open on deactivation.
- **Snapshot.** The list is a snapshot of the keymap and actions taken when the dialog opens. The webview re-renders on keymap changes made while it is open.
- **Titles of unregistered actions (assumption).** A bound action that is not registered here is titled and categorised from `assets/gen/actions.json` (the actions the webview registers), not humanised from its id. In the webview every such action is registered, so this matches what it shows. Ids found in neither place are still humanised by `app_models::shell`.

---

## Part B: tabs in a single-file window (LightEdit)

### Design

`Content::Editor` becomes a struct variant with exactly the tab fields of `Content::Project`, with the same names and types. Each guard that reads the tab parts of `Content::Project` then becomes an or-pattern, and a light-edit window reuses the project machinery: the tab strip and editor area (`editor_area` / `pane_element`), `with_tabs`, `tabs_changed` (including its save-or-ask on closing a dirty tab, and the restore on failure), the tab menu, `load_tab_probed`, viewer and Markdown tabs, splits, reveal, and Find bars per tab.

`light_find_bar` is removed everywhere. Find bars live in `find_bars`, per tab, as in a project.

**Or-pattern rule.**

- In `let … else` the alternatives need parentheses.
- In `if let` and in `match` arms they do not. Adding parentheses there triggers `unused_parens`.

### B1. `app-gpui/src/workbench/mod.rs`: the enum

Replace

```rust
    Opening(PathBuf),
    Editor(Entity<Editor>),
```

with

```rust
    Opening(PathBuf),
    /// A window that edits loose files on its own (LightEdit): editor tabs without a project, kept
    /// with the same parts as a project's tabs.
    Editor {
        tabs: Box<TabsState>,
        editors: HashMap<String, Entity<Editor>>,
        loading: std::collections::HashSet<String>,
        restored_views: std::collections::BTreeMap<String, FileViewState>,
        reveals: HashMap<String, (u32, u32)>,
        editor_focus: HashMap<String, Subscription>,
        find_bars: HashMap<String, TabFindBar>,
        view_tabs: tab_views::ViewTabs,
    },
```

**Compile risk.** The enum carries `#[expect(clippy::large_enum_variant, reason = …)]`. With two large variants, clippy may stop firing (then `unfulfilled_lint_expectations`: remove the `#[expect]`), or it may start naming `Editor` (keep it). Let the compiler decide.

### B2. `mod.rs`: the `Workbench` field

Replace

```rust
    /// The Find / Replace bar of a single-file window's editor.
    light_find_bar: Option<TabFindBar>,
```

with

```rust
    /// The loose files waiting for the window's first one to open, while it turns into LightEdit.
    light_pending: Option<Vec<crate::app_models::windows::FileOpen>>,
```

In `Workbench::new` replace `            light_find_bar: None,` with `            light_pending: None,`.

Replace both remaining `        self.light_find_bar = None;` lines (in `close_project` and `open_project`) with `        self.light_pending = None;`.

### B3. `mod.rs`: `for_each_intelligence` and `editors()`

The same two lines appear in both functions. Replace both occurrences of

```rust
            Content::Editor(editor) => vec![editor.clone()],
            Content::Project { editors, .. } => editors.values().cloned().collect(),
```

with

```rust
            Content::Project { editors, .. } | Content::Editor { editors, .. } => editors.values().cloned().collect(),
```

### B4. `mod.rs`: `editor()` and `tabs()`

Replace

```rust
            Content::Editor(editor) => Some(editor),
            Content::Project { tabs, editors, .. } => {
```

with

```rust
            Content::Project { tabs, editors, .. } | Content::Editor { tabs, editors, .. } => {
```

Replace

```rust
    /// The editor tabs, when a project is open.
    pub fn tabs(&self) -> Option<&TabsState> {
        match &self.content {
            Content::Project { tabs, .. } => Some(tabs.as_ref()),
```

with

```rust
    /// The editor tabs, of a project or of a window that edits loose files.
    pub fn tabs(&self) -> Option<&TabsState> {
        match &self.content {
            Content::Project { tabs, .. } | Content::Editor { tabs, .. } => Some(tabs.as_ref()),
```

### B5. `mod.rs`: `pane_element`

Replace

```rust
            Content::Project { find_bars, .. } if editor.is_some() => {
```

with

```rust
            Content::Project { find_bars, .. } | Content::Editor { find_bars, .. } if editor.is_some() => {
```

Replace

```rust
            (None, None, None) if shown_tab.is_none() => Some(empty_editor::empty_editor(cx).into_any_element()),
```

with

```rust
            (None, None, None) if shown_tab.is_none() => {
                // A window that edits loose files has no project to navigate: it offers Open File.
                let hints = if matches!(self.content, Content::Editor { .. }) {
                    empty_editor::light_edit_empty_editor(cx)
                } else {
                    empty_editor::empty_editor(cx)
                };
                Some(hints.into_any_element())
            }
```

### B6. `mod.rs`: `with_tabs`, `tabs_changed`, `restore_unsaved`

In `with_tabs` replace

```rust
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        if change(tabs, &pane) {
```

with

```rust
        let (Content::Project { tabs, .. } | Content::Editor { tabs, .. }) = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        if change(tabs, &pane) {
```

In `tabs_changed` replace

```rust
        let Content::Project { tabs, editors, editor_focus, find_bars, .. } = &mut self.content else { return };
```

with

```rust
        let (Content::Project { tabs, editors, editor_focus, find_bars, .. }
        | Content::Editor { tabs, editors, editor_focus, find_bars, .. }) = &mut self.content
        else {
            return;
        };
```

In the same function, replace

```rust
                        let saving = editor.read_with(cx, |editor, cx| editor.save(cx));
                        match saving.await {
                            Ok(Ok(())) => return,
```

with

```rust
                        let saving = editor.read_with(cx, |editor, cx| editor.save(cx));
                        match saving.await {
                            Ok(Ok(())) => return light_edit_tabs::closed_tab_settled(&workbench, cx),
```

and

```rust
                    // Don't Save.
                    Some(1) => return,
```

with

```rust
                    // Don't Save.
                    Some(1) => return light_edit_tabs::closed_tab_settled(&workbench, cx),
```

At the end of `tabs_changed` replace

```rust
        cx.notify();
    }

    /// A closed tab whose document was not saved, because saving failed or the user chose to
```

with

```rust
        // A window that edits loose files closes with its last tab, once no closed tab waits on a save.
        self.close_emptied_light_edit(closing.is_empty(), window, cx);
        cx.notify();
    }

    /// A closed tab whose document was not saved, because saving failed or the user chose to
```

In `restore_unsaved` replace

```rust
        let Content::Project { tabs, editors, find_bars, view_tabs, .. } = &mut self.content else { return };
```

with

```rust
        let (Content::Project { tabs, editors, find_bars, view_tabs, .. }
        | Content::Editor { tabs, editors, find_bars, view_tabs, .. }) = &mut self.content
        else {
            return;
        };
```

### B7. `mod.rs`: `publish_context`

Replace

```rust
        set_context(window, cx, "lightEdit", matches!(self.content, Content::Editor(_) | Content::Viewer(_)));
```

with

```rust
        set_context(window, cx, "lightEdit", matches!(self.content, Content::Editor { .. } | Content::Viewer(_)));
```

Replace

```rust
        // A window that edits a loose file on its own shows it as its one tab.
        let tabs_open = matches!(self.content, Content::Editor(_) | Content::Viewer(_))
```

with

```rust
        // A loose file shown in a viewer is the window's one tab.
        let tabs_open = matches!(self.content, Content::Viewer(_))
```

### B8. `mod.rs`: Recent Files, reveal, the tab menu

In the Recent Files events replace

```rust
                    if let Content::Project { tabs, .. } = &mut workbench.content {
                        tabs.close_key(&key);
```

with

```rust
                    if let Content::Project { tabs, .. } | Content::Editor { tabs, .. } = &mut workbench.content {
                        tabs.close_key(&key);
```

In `open_project_file_at` replace

```rust
        if let Content::Project { reveals, .. } = &mut self.content {
```

with

```rust
        if let Content::Project { reveals, .. } | Content::Editor { reveals, .. } = &mut self.content {
```

In `tab_menu` replace

```rust
                Content::Project { editors, .. } => editors.get(tab).map(|editor| editor.read(cx).text()),
```

with

```rust
                Content::Project { editors, .. } | Content::Editor { editors, .. } => {
                    editors.get(tab).map(|editor| editor.read(cx).text())
                }
```

In `read_only_tabs` replace

```rust
            Content::Project { editors, .. } => {
                editors.iter().filter(|(_, editor)| editor.read(cx).is_read_only()).map(|(id, _)| id.clone()).collect()
```

with

```rust
            Content::Project { editors, .. } | Content::Editor { editors, .. } => {
                editors.iter().filter(|(_, editor)| editor.read(cx).is_read_only()).map(|(id, _)| id.clone()).collect()
```

In `run_tab_command` replace

```rust
            Content::Project { editors, view_tabs, .. } => editors
```

with

```rust
            Content::Project { editors, view_tabs, .. } | Content::Editor { editors, view_tabs, .. } => editors
```

### B9. `mod.rs`: `serve_reveal`, `reopen_closed_tab`, `load_active_tab`, `editor_took_focus`, `split_editor`

In `serve_reveal` replace

```rust
        let Content::Project { reveals, .. } = &mut self.content else { return };
```

with

```rust
        let (Content::Project { reveals, .. } | Content::Editor { reveals, .. }) = &mut self.content else { return };
```

In `reopen_closed_tab` replace

```rust
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let Some(reopened) = tabs.reopen_closed() else { return };
```

with

```rust
        let (Content::Project { tabs, .. } | Content::Editor { tabs, .. }) = &mut self.content else { return };
        let Some(reopened) = tabs.reopen_closed() else { return };
```

In `load_active_tab` replace

```rust
        let Content::Project { tabs, editors, loading, .. } = &self.content else { return };
```

with

```rust
        let (Content::Project { tabs, editors, loading, .. } | Content::Editor { tabs, editors, loading, .. }) =
            &self.content
        else {
            return;
        };
```

In `editor_took_focus` replace

```rust
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let Some(pane) = tabs.panes.iter().find(|pane| pane.tab(tab).is_some()).map(|pane| pane.id.clone()) else {
```

with

```rust
        let (Content::Project { tabs, .. } | Content::Editor { tabs, .. }) = &mut self.content else { return };
        let Some(pane) = tabs.panes.iter().find(|pane| pane.tab(tab).is_some()).map(|pane| pane.id.clone()) else {
```

In `split_editor` replace

```rust
        let Content::Project { tabs, reveals, .. } = &mut self.content else { return };
```

with

```rust
        let (Content::Project { tabs, reveals, .. } | Content::Editor { tabs, reveals, .. }) = &mut self.content
        else {
            return;
        };
```

### B10. `mod.rs`: `save_session`

Replace

```rust
        self.session_save = None;
        let Content::Project { tabs, editors, restored_views, layout, .. } = &self.content else { return };
```

with

```rust
        self.session_save = None;
        if matches!(self.content, Content::Editor { .. }) {
            return self.save_light_session(cx);
        }
        let Content::Project { tabs, editors, restored_views, layout, .. } = &self.content else { return };
```

Update the doc above it: "Stores the project's editor session now" becomes "Stores the window's editor session now (a project's, or the LightEdit one)".

### B11. `mod.rs`: `load_tab_probed`

Six guards.

Replace

```rust
        let Content::Project { loading, editors, .. } = &mut self.content else { return };
```

with

```rust
        let (Content::Project { loading, editors, .. } | Content::Editor { loading, editors, .. }) = &mut self.content
        else {
            return;
        };
```

Replace both occurrences, at two different indentations, of

```rust
if let Content::Project { loading, .. } = &mut workbench.content {
```

with

```rust
if let Content::Project { loading, .. } | Content::Editor { loading, .. } = &mut workbench.content {
```

Replace

```rust
                        if let (true, Content::Project { editors, restored_views, .. }) =
```

with

```rust
                        if let (
                            true,
                            Content::Project { editors, restored_views, .. }
                            | Content::Editor { editors, restored_views, .. },
                        ) =
```

Replace

```rust
                            if let Content::Project { editor_focus, .. } = &mut workbench.content {
```

with

```rust
                            if let Content::Project { editor_focus, .. } | Content::Editor { editor_focus, .. } =
                                &mut workbench.content
                            {
```

Replace

```rust
                        if let Content::Project { tabs, .. } = &mut workbench.content {
                            let pane =
```

with

```rust
                        if let Content::Project { tabs, .. } | Content::Editor { tabs, .. } = &mut workbench.content {
                            let pane =
```

### B12. `mod.rs`: `render`, and a test

Replace

```rust
            Content::Editor(editor) => self.light_editor_element(editor, cx),
```

with

```rust
            Content::Editor { tabs, .. } => self.editor_area(&tabs.layout, window, cx),
```

In `mod tests`:

- Replace `matches!(w.content, Content::Editor(_))` with `matches!(w.content, Content::Editor { .. })`.
- After `    mod proofs13_windows;` add `    mod proofs13_windows_pending;`. You may rename the file to drop `_pending`.

### B13. `app-gpui/src/workbench/tab_views.rs`

In `tab_content` replace

```rust
        let Content::Project { editors, view_tabs, .. } = &self.content else { return None };
```

with

```rust
        let (Content::Project { editors, view_tabs, .. } | Content::Editor { editors, view_tabs, .. }) = &self.content
        else {
            return None;
        };
```

In `active_tab_view` replace

```rust
            Content::Project { tabs, .. } => {
                let id = tabs.pane(&tabs.active_pane)?.active.as_deref()?;
```

with

```rust
            Content::Project { tabs, .. } | Content::Editor { tabs, .. } => {
                let id = tabs.pane(&tabs.active_pane)?.active.as_deref()?;
```

In `markdown_host` replace

```rust
        let Content::Project { view_tabs, .. } = &self.content else { return None };
```

with

```rust
        let (Content::Project { view_tabs, .. } | Content::Editor { view_tabs, .. }) = &self.content else {
            return None;
        };
```

In `open_as_text` replace

```rust
        if matches!(self.content, Content::Project { .. }) {
```

with

```rust
        if matches!(self.content, Content::Project { .. } | Content::Editor { .. }) {
```

In `open_text_tab` replace

```rust
        let Content::Project { tabs, editors, .. } = &mut self.content else { return };
        let key = path.to_string_lossy().into_owned();
```

with

```rust
        let (Content::Project { tabs, editors, .. } | Content::Editor { tabs, editors, .. }) = &mut self.content else {
            return;
        };
        let key = path.to_string_lossy().into_owned();
```

In `tab_shows_viewer` replace

```rust
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let Some(pane) = pane_of(tabs, tab_id) else { return };
```

with

```rust
        let (Content::Project { tabs, .. } | Content::Editor { tabs, .. }) = &mut self.content else { return };
        let Some(pane) = pane_of(tabs, tab_id) else { return };
```

In `build_view_tab` replace

```rust
                if let Content::Project { tabs, .. } = &mut self.content
                    && let Some(pane) = pane_of(tabs, &tab_id)
```

with

```rust
                if let Content::Project { tabs, .. } | Content::Editor { tabs, .. } = &mut self.content
                    && let Some(pane) = pane_of(tabs, &tab_id)
```

In `install_view` replace

```rust
        if let Content::Project { view_tabs, editor_focus, .. } = &mut self.content {
```

with

```rust
        if let Content::Project { view_tabs, editor_focus, .. } | Content::Editor { view_tabs, editor_focus, .. } =
            &mut self.content
        {
```

In `release_closed_views` replace

```rust
        let Content::Project { tabs, view_tabs, editor_focus, .. } = &mut self.content else { return };
```

with

```rust
        let (Content::Project { tabs, view_tabs, editor_focus, .. }
        | Content::Editor { tabs, view_tabs, editor_focus, .. }) = &mut self.content
        else {
            return;
        };
```

In `restore_view` replace

```rust
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        let Some(id) = tabs.open_tab(&pane, view.input.clone(), OpenTabOptions::default()) else { return };
```

with

```rust
        let (Content::Project { tabs, .. } | Content::Editor { tabs, .. }) = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        let Some(id) = tabs.open_tab(&pane, view.input.clone(), OpenTabOptions::default()) else { return };
```

In `wrap_markdown` replace

```rust
        if let Content::Project { view_tabs, .. } = &mut self.content {
            view_tabs.markdown.insert(
```

with

```rust
        if let Content::Project { view_tabs, .. } | Content::Editor { view_tabs, .. } = &mut self.content {
            view_tabs.markdown.insert(
```

Replace `open_file`:

```rust
    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_loose_file(path, false, window, cx);
    }
```

with

```rust
    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // A window that edits loose files takes another one as a tab.
        if self.open_light_tab(path.clone(), None, window, cx) {
            return;
        }
        self.open_loose_file(path, false, window, cx);
    }
```

Update its doc: "Opens a loose file for editing (LightEdit: no project, no tool windows)" becomes "Opens a loose file for editing (LightEdit: no project, no tool windows): as a tab when the window edits loose files already".

In `open_loose_file` replace

```rust
        self.content = Content::Opening(path.clone());
        self.light_find_bar = None;
```

with

```rust
        self.content = Content::Opening(path.clone());
        self.light_pending = None;
```

Replace

```rust
                                workbench.observe_editor(&editor, cx);
                                Content::Editor(editor)
```

with

```rust
                                workbench.observe_editor(&editor, cx);
                                workbench.light_edit_content(editor, window, cx)
```

Replace

```rust
                            None => Content::Failed(input.key.into()),
                        }
                    }
                    Err(message) => Content::Failed(message.into()),
                };
                workbench.sync_welcome(window, cx);
```

with

```rust
                            None => Content::Failed(input.key.into()),
                        }
                    }
                    Err(message) => Content::Failed(message.into()),
                };
                workbench.sync_welcome(window, cx);
                workbench.light_edit_opened(window, cx);
```

### B14. `app-gpui/src/workbench/editor_find.rs` (removing `light_find_bar`)

Module doc: "one per editor tab of a project, and one over the editor of a single-file window" becomes "one per editor tab, of a project or of a window that edits loose files".

Delete the field `TabFindBar::editor` and its doc line:

```rust
    /// The editor the bar searches.
    editor: Entity<Editor>,
```

Replace `        let open = TabFindBar { bar, editor, _subscriptions: subscriptions };` with `        let open = TabFindBar { bar, _subscriptions: subscriptions };`. The local `editor` is still used above that line.

Delete the whole function `light_find_bar`:

```rust
    /// The bar of the single-file window's editor, when it has one open.
    fn light_find_bar(&self) -> Option<&TabFindBar> {
        let Content::Editor(editor) = &self.content else { return None };
        self.light_find_bar.as_ref().filter(|bar| &bar.editor == editor)
    }
```

In `find_bar` replace

```rust
            Content::Project { find_bars, .. } => find_bars.get(self.active_tab_id()?).map(|bar| bar.bar.clone()),
            Content::Editor(_) => self.light_find_bar().map(|bar| bar.bar.clone()),
```

with

```rust
            Content::Project { find_bars, .. } | Content::Editor { find_bars, .. } => {
                find_bars.get(self.active_tab_id()?).map(|bar| bar.bar.clone())
            }
```

In `shown_find_bars`:

- Replace `            Content::Project { tabs, find_bars, .. } => tabs` with `            Content::Project { tabs, find_bars, .. } | Content::Editor { tabs, find_bars, .. } => tabs`.
- Delete `            Content::Editor(_) => self.light_find_bar().map(|bar| bar.bar.clone()).into_iter().collect(),`.

In `show_find_bar` replace

```rust
        // A project's bars are kept by tab; a single-file window has one editor.
        let tab = match &self.content {
            Content::Project { .. } => match self.active_tab_id() {
```

with

```rust
        // The bars are kept by tab.
        let tab = match &self.content {
            Content::Project { .. } | Content::Editor { .. } => match self.active_tab_id() {
```

and

```rust
        match (&mut self.content, tab) {
            (Content::Project { find_bars, .. }, Some(tab)) => {
                find_bars.insert(tab, open);
            }
            _ => self.light_find_bar = Some(open),
        }
```

with

```rust
        if let (Content::Project { find_bars, .. } | Content::Editor { find_bars, .. }, Some(tab)) =
            (&mut self.content, tab)
        {
            find_bars.insert(tab, open);
        }
```

In `find_bar_event` replace

```rust
                    (Content::Project { find_bars, .. }, Some(tab)) => {
                        find_bars.get(tab).is_some_and(|open| &open.bar == bar) && find_bars.remove(tab).is_some()
                    }
                    _ => {
                        self.light_find_bar.as_ref().is_some_and(|open| &open.bar == bar)
                            && self.light_find_bar.take().is_some()
                    }
```

with

```rust
                    (Content::Project { find_bars, .. } | Content::Editor { find_bars, .. }, Some(tab)) => {
                        find_bars.get(tab).is_some_and(|open| &open.bar == bar) && find_bars.remove(tab).is_some()
                    }
                    _ => false,
```

**Delete `light_editor_element`.** Remove the whole function, from its doc line `    /// The single-file window's editor, under its bar when one is open.` to its closing brace. Its body is being edited by another agent (an editor context menu). Remove it whatever it contains: `pane_element` already opens the editor context menu on a right click, for every pane.

Then drop the imports the compiler reports unused. With the file as it is now, that is `AnyElement`, `IntoElement`, `div`, `prelude::*` and `Editor`.

### B15. `app-gpui/src/workbench/app_shell.rs`

In `window_showing` replace

```rust
                || workbench.light_edit_file(cx).as_deref() == Some(file)
```

with

```rust
                || workbench.light_edit_shows(file)
```

In `is_light_edit` replace `        matches!(self.content, Content::Editor(_) | Content::Viewer(_))` with `        matches!(self.content, Content::Editor { .. } | Content::Viewer(_))`.

In `light_edit_file` replace

```rust
            Content::Editor(editor) => Some(editor.read(cx).path().to_path_buf()),
```

with

```rust
            // The active tab's file, known before its document has opened.
            Content::Editor { .. } => self
                .editor()
                .map(|editor| editor.read(cx).path().to_path_buf())
                .or_else(|| self.light_edit_active_path().map(PathBuf::from)),
```

In `open_result` replace `            Content::Empty | Content::Editor(_) | Content::Viewer(_) => {` with `            Content::Empty | Content::Editor { .. } | Content::Viewer(_) => {`.

In `close_to_welcome` replace `        self.light_find_bar = None;` with `        self.light_pending = None;`.

In `close_light_edit_tab` replace

```rust
    if !crate::app_models::windows::light_edit_should_close(false, workbench.read(cx).is_light_edit(), 0) {
```

with

```rust
    let open_tabs = workbench.read(cx).tabs().map_or(0, |tabs| tabs.open_keys().len());
    if !crate::app_models::windows::light_edit_should_close(false, workbench.read(cx).is_light_edit(), open_tabs) {
```

Its doc becomes: "Close Tab in a window that edits loose files on its own and has no tab left (a viewer's file, or the tabs are all closed): the window closes through its close guard (the last window shows the Welcome screen instead). Returns whether it was such a window."

In `open_files` replace

```rust
            LooseStep::OpenLightEdit { path } => {
                workbench.update(cx, |workbench, cx| workbench.open_file(PathBuf::from(path), window, cx));
```

with

```rust
            LooseStep::OpenLightEdit { path } => {
                workbench.update(cx, |workbench, cx| {
                    workbench.open_file(PathBuf::from(path), window, cx);
                    // The files that follow open as its tabs once it has opened.
                    workbench.queue_light_tabs();
                });
```

and

```rust
                let shown = workbench.read(cx).light_edit_file(cx);
                for open in opens.into_iter().filter(|open| shown.as_deref() != Some(Path::new(&open.path))) {
                    windows::open_window(Target::File(PathBuf::from(open.path)), cx)?;
                }
                return Ok(());
```

with

```rust
                // A window that edits loose files takes them as tabs; one showing a viewer cannot.
                let separate = workbench.update(cx, |workbench, cx| workbench.take_loose_files(opens, window, cx));
                for open in separate {
                    windows::open_window(Target::File(PathBuf::from(open.path)), cx)?;
                }
                return Ok(());
```

Its doc becomes: "… otherwise a file of a project open in another window opens there, the rest open as tabs of the window that edits loose files: this one, the one that already does when this one shows nothing, or this one turning into one."

In `route_files` replace

```rust
                    if workbench.project_tree().is_some() {
                        workbench.open_project_file(path, window, cx);
                    }
```

with

```rust
                    if workbench.project_tree().is_some() {
                        workbench.open_project_file(path, window, cx);
                    } else {
                        // A window that edits loose files brings the file's tab to the front.
                        workbench.open_light_tab(path, None, window, cx);
                    }
```

At the end of `route_files` (the only `    left\n}` in the file) replace

```rust
    }
    left
}
```

with

```rust
    }
    // From a window that shows nothing, the rest go to the window that edits loose files.
    let asking_is_empty = for_window(window, cx).is_some_and(|workbench| workbench.read(cx).is_empty());
    if !left.is_empty()
        && asking_is_empty
        && let Some(light) = super::light_edit_tabs::light_edit_window(here, cx)
    {
        let _ = light.update(cx, |_, window, cx| {
            window.activate_window();
            if let Some(workbench) = for_window(window, cx) {
                workbench.update(cx, |workbench, cx| {
                    for file in left {
                        workbench.open_light_tab(PathBuf::from(file), None, window, cx);
                    }
                });
            }
        });
        return Vec::new();
    }
    left
}
```

If `left` is reported moved into the closure while it is still borrowed by `!left.is_empty()`, that is not a real conflict: the condition is evaluated first.

`Path` is still used in `app_shell.rs` by `canonical` and `window_with_project`.

### B16. `app-gpui/src/workbench/chrome.rs`

In `sync_chrome` replace

```rust
        let light_edit = matches!(self.content, Content::Editor(_) | Content::Viewer(_));
```

with

```rust
        let light_edit = matches!(self.content, Content::Editor { .. } | Content::Viewer(_));
```

In `sync_title_bar` replace

```rust
            Content::Editor(editor) => {
                let name = editor.read(cx).path().file_name().map(|name| name.to_string_lossy().into_owned());
```

with

```rust
            Content::Editor { .. } => {
                // The active tab's file name.
                let name = self.light_edit_title();
```

### B17. `app-gpui/src/workbench/editor_core.rs`

In `window_editors` replace

```rust
            Content::Project { editors, .. } => editors.values().cloned().collect(),
            Content::Editor(editor) => vec![editor.clone()],
```

with

```rust
            Content::Project { editors, .. } | Content::Editor { editors, .. } => editors.values().cloned().collect(),
```

In `go_to_place` replace

```rust
            Content::Project { .. } => {
                self.open_project_file_at(PathBuf::from(place.path), place.line, place.col, window, cx);
            }
            Content::Editor(editor) if editor.read(cx).path().to_string_lossy() == place.path => {
                let editor = editor.clone();
                editor.update(cx, |editor, cx| {
                    let offset = editor.offset_of(place.line, place.col);
                    editor.select(offset, offset, cx);
                });
                window.focus(&editor.focus_handle(cx), cx);
            }
```

with

```rust
            Content::Project { .. } | Content::Editor { .. } => {
                self.open_project_file_at(PathBuf::from(place.path), place.line, place.col, window, cx);
            }
```

Drop the imports the compiler then reports unused. Probably none.

### B18. `app-gpui/src/workbench/editor_intelligence.rs`

In `open_location` replace

```rust
            Content::Project { .. } => self.open_project_file_at(path, start.line, start.col, window, cx),
            Content::Editor(editor) if editor.read(cx).path() == path.as_path() => {
                let editor = editor.clone();
                editor.update(cx, |editor, cx| {
                    let offset = pos_to_offset(editor.rope(), start);
                    editor.select(offset, offset, cx);
                });
            }
```

with

```rust
            Content::Project { .. } | Content::Editor { .. } => {
                self.open_project_file_at(path, start.line, start.col, window, cx);
            }
```

Its doc becomes "… in its tab (of the project, or of the window that edits loose files) …".

If `pos_to_offset` is reported unused, drop it from the imports.

### B19. `app-gpui/src/workbench/project_files.rs`

In `close_removed_tabs` replace

```rust
        let Content::Project { tabs, editors, .. } = &mut self.content else { return };
        let dirty = |id: &str|
```

with

```rust
        let (Content::Project { tabs, editors, .. } | Content::Editor { tabs, editors, .. }) = &mut self.content else {
            return;
        };
        let dirty = |id: &str|
```

### B20. `app-gpui/src/workbench/empty_editor.rs`

Replace

```rust
/// The hints with their shortcuts as the keymap labels them, in order.
pub fn hint_rows(cx: &App) -> Vec<HintRow> {
    let keymap = cx.try_global::<Keymap>();
    let registry = cx.try_global::<ActionRegistry>();
    HINTS
        .iter()
```

with

```rust
/// A window that edits loose files has no project to navigate: only opening files works there.
const LIGHT_EDIT_HINTS: [Hint; 2] = [
    Hint { label: "Open File", action: Some("file.openFile"), fallback: None },
    Hint { label: "Drop files here to open them", action: None, fallback: None },
];

/// The hints with their shortcuts as the keymap labels them, in order.
pub fn hint_rows(cx: &App) -> Vec<HintRow> {
    rows_of(&HINTS, cx)
}

/// The hints of an empty pane of a window that edits loose files.
pub fn light_edit_hint_rows(cx: &App) -> Vec<HintRow> {
    rows_of(&LIGHT_EDIT_HINTS, cx)
}

fn rows_of(hints: &[Hint], cx: &App) -> Vec<HintRow> {
    let keymap = cx.try_global::<Keymap>();
    let registry = cx.try_global::<ActionRegistry>();
    hints
        .iter()
```

Replace

```rust
/// The empty editor, filling its pane, the hints in the middle.
pub fn empty_editor(cx: &App) -> Stateful<Div> {
    let mut rows = hint_rows(cx);
```

with

```rust
/// The empty editor, filling its pane, the hints in the middle.
pub fn empty_editor(cx: &App) -> Stateful<Div> {
    hints_element(hint_rows(cx), cx)
}

/// The empty pane of a window that edits loose files: Open File, and the remark on dropping files.
pub fn light_edit_empty_editor(cx: &App) -> Stateful<Div> {
    hints_element(light_edit_hint_rows(cx), cx)
}

fn hints_element(mut rows: Vec<HintRow>, cx: &App) -> Stateful<Div> {
```

---

## Every `Content::Editor` match today, and what it does now

Line numbers are as read now. They drift.

| Site                                        | What it does now                                   |
| ------------------------------------------- | -------------------------------------------------- |
| `app_shell.rs:194` `is_light_edit`          | `{ .. }`                                           |
| `app_shell.rs:200` `light_edit_file`        | the active tab's file                              |
| `app_shell.rs:215` `open_result`            | `{ .. }`                                           |
| `chrome.rs:216`                             | `{ .. }`                                           |
| `chrome.rs:289` title bar                   | the active tab's name                              |
| `editor_core.rs:183` `window_editors`       | every tab's editor                                 |
| `editor_core.rs:198` `go_to_place`          | opens or brings the tab to the front, then reveals |
| `editor_find.rs:28/37/51`                   | per-tab find bars                                  |
| `editor_intelligence.rs:63` `open_location` | as a tab, revealed                                 |
| `tab_views.rs:885`                          | builds the tab content                             |
| `mod.rs:1333` `for_each_intelligence`       | every tab                                          |
| `mod.rs:1346` `editor()`                    | the active tab's editor                            |
| `mod.rs:1745` `editors()`                   | every tab                                          |
| `mod.rs:2079` `lightEdit`                   | `{ .. }`                                           |
| `mod.rs:2086` `editorTabsOpen`              | real tabs; a viewer counts as one                  |
| `mod.rs:3881` `render`                      | `editor_area` with the strip                       |
| `mod.rs:11983` test                         | `{ .. }`                                           |

## Not built, and why

- **A viewer window takes no second file.** A window whose first file shows in a viewer (`Content::Viewer`, an image) still takes no tabs: a second loose file opens its own window, as before. A viewer file opened into a tabbed window becomes a tab.
- **Dropping files.** There is no handler for files dropped on a window, so the empty pane's "Drop files here" is a remark only (it is one in the webview's text too).
- **Debugger and plugin gutters.** The first tab of a light window gets no `watch_debug_editor` / `watch_plugin_gutter`, as before. Later tabs get them through `load_tab_probed`.

- **A loose tab that cannot open is only logged.** It goes away again, and `load_tab_probed` logs the failure as it does for a project. The webview raises `openFailed` (`app.window.openFailed`) for it.
- **The light empty pane has no end-to-end test.** It is reached while a closed last tab waits on its Save question, or when the close request is refused. `light_edit_tabs::an_empty_light_edit_pane_offers_open_file_and_dropping` tests only its rows.

## IPC commands behind the actions

- `file.openFile` (and loose files routed to the window): `window_open_files` (the routing, done natively by `route_files`), `light_edit_open` (`Host::open_light`, first file only), `fs_probe` and `doc_open` (`ide_host::ops::bytes::fs_probe`, `ide_host::ops::doc::open`, per tab through `load_tab_probed`).
- `editor.closeTab` and `editor.closeOtherTabs`: no IPC, except `doc_save` for a dirty closed tab, and `window_close` through the close guard for the last tab.
- `help.keymapReference`: none.

## Assumptions to check first

- **Verified by reading.** Without a project these return early or are harmless:
  - `project_view_tabs_changed`: `let Some(tree) = self.project_tree()`;
  - `plugin_tabs_changed`: reports only with a `project_root`;
  - `watch_debug_editor`: `let Some(slot) = self.debug`;
  - `watch_plugin_gutter`: needs `Plugins`;
  - `markdown_shared`;
  - `mark_recent`: a failure is logged at debug level.
- **No other `tabs()` caller assumes a project.** None of the callers of `tabs()` / `with_tabs(` in non-test code treats `Some` as "a project is open". Only tests on project windows `expect("a project is open")`.
- **Session timing.** The light session (`<lightEdit>`) is written by the 400 ms `session_save` task, by `leave()`, and when the last tab closes. Existing tests that turn two windows into LightEdit one after the other in one test could now see restored tabs, but only once no other light window is open.

## Compile risks

- `#[expect(clippy::large_enum_variant)]` on `Content` (B1).
- Missing any `Content::Editor(` or `light_find_bar` site. The compiler lists them all.
- Imports reported unused after B14, B17 and B18.
- The `Some(1) => return light_edit_tabs::closed_tab_settled(&workbench, cx)` arms return `()` inside `let failure = match …`. They diverge as `return` does.
