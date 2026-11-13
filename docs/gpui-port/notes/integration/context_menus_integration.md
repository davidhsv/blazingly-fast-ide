# Integrating the context menus built from the menu groups

New file, not compiled yet: `app-gpui/src/workbench/context_menus.rs`. It needs the draft `app-gpui/src/workbench/plugin_contributions.rs` compiled and declared first: it calls `super::plugin_contributions::{PluginMenu, plugin_menu_items, plugin_slot_items}`. Every anchor below is quoted from the file as it is now; line numbers are left out. Several pasted lines run past 120 columns: run `cargo fmt` after pasting.

Sections 1 to 6 are all required: each `pub(super)` method of `context_menus.rs` is called only from a hunk, so a partial apply fails `-D warnings` on dead code (and 1i is needed for it to compile at all).

No action is registered by this work and no shortcut is bound. The only backend call is the settings write of the gutter's Appearance switches and of Show Tool Window Names (`crate::platform::settings::set`, IPC `settings_set`); every other row runs a registered action.

## 0. Corrections to the brief (from reading the code, not from a run)

- The editor tab already has a context menu: `Workbench::tab_menu`, `TabCommand`, `run_tab_command` in `workbench/mod.rs`, opened by right click on a tab, with the test `a_tabs_menu_closes_pins_and_splits`. This work adds what it lacks (section 1e), it does not rebuild it.
- Problems already has its menu (`ProblemsViewEvent::ShowMenu`); only the `problems` groups are added (section 2).
- A native model of the groups already exists: `run_producers/context_run.rs` `ContextMenu`, `MenuGroupContext`, `MenuGroup`, `MenuGroups` (order, then `collate` by id, a separator after each non-empty group). `context_menus.rs` uses it; nothing is duplicated.
- `assets/gen/contributions.json` carries each group's `id`, `menus`, `order` and `source`, but its `items` are `"$items": "function"` (no rows). The rows are therefore Rust functions (`MENU_GROUPS` in `context_menus.rs`).
- The webview opens the editor menu only from the DOM `contextmenu` event (`src/editor/core/controller.ts` `onContextMenu`); there is no keymap binding for it (Shift+F10 is `run.run` in the Windows keymaps). No keyboard opening is built.
- The menu bar and the popup menus treat an unregistered action differently. `popup_menu::resolve` leaves its row out (as `src/ui/Menu.tsx` `resolveMenuItems` does). `menu::state` only greys out registered actions that cannot run, so the bar shows an unregistered action as enabled. The context menus follow `resolve` (the webview's behaviour). The menu bar's difference is not changed here. Check: `menu.rs` `state` builds `disabled` only from `registry.get(id).is_some_and(…)`, and `item_enabled` is true for an id not in that set.

## 1. `app-gpui/src/workbench/mod.rs`

### 1a. The module

After `mod conflicts;` add:

```rust
pub mod context_menus;
```

It is `pub` because `vcs/commit_box.rs` calls `crate::workbench::context_menus::slot_buttons`.

### 1b. The editor's right click (editor and gutter menus)

In `fn pane_element`, directly before `        let body: Option<gpui_kit::AnyElement> = match (view, host, editor) {` add:

```rust
        // A right click in the editor opens its menu (left of the text: the gutter's).
        let menu_editor = editor.clone();
```

Then replace

```rust
            .child(div().flex_1().min_h_0().children(body))
```

with

```rust
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when_some(menu_editor, |el, editor| {
                        el.on_mouse_down(
                            gpui_kit::MouseButton::Right,
                            cx.listener(move |workbench, event: &MouseDownEvent, window, cx| {
                                workbench.open_editor_context_menu(&editor, event.position, window, cx);
                                cx.stop_propagation();
                            }),
                        )
                    })
                    .children(body),
            )
```

`editor/view.rs` handles only the left button, so the press reaches this wrapper. The editor's own `track_focus` focuses it on the press; `open_editor_context_menu` focuses it again, places the caret, publishes the context and opens the menu.

### 1c. The `editor.title` slot in the tab strip

In `fn pane_element`, replace

```rust
                    .child(div().flex_1())
                    .children(layout_actions)
```

with

```rust
                    .child(div().flex_1())
                    .children(context_menus::slot_buttons("editor.title", window, cx))
                    .children(layout_actions)
```

### 1d. `TabCommand::CloseReadOnly`

In `pub enum TabCommand {`, after `    CloseAllButPinned,` add:

```rust
    CloseReadOnly,
```

### 1e. The tab menu's missing rows

1. Replace the signature line

```rust
    pub fn tab_menu(&self, pane: &str, tab: &str, cx: &App) -> Vec<MenuItem<TabCommand>> {
```

with

```rust
    pub fn tab_menu(&self, pane: &str, tab: &str, window: &Window, cx: &App) -> Vec<MenuItem<TabCommand>> {
```

2. Replace

```rust
        if open.tabs.iter().any(|other| other.pinned) {
            items.push(row("Close All but Pinned", TabCommand::CloseAllButPinned, true));
        }
```

with

```rust
        if open.tabs.iter().any(|other| other.pinned) {
            items.push(row("Close All but Pinned", TabCommand::CloseAllButPinned, true));
        }
        let read_only = self.read_only_tabs(cx);
        if open.tabs.iter().any(|other| !other.pinned && other.input.path.is_some() && read_only.contains(&other.id)) {
            items.push(row("Close All Read-Only Tabs", TabCommand::CloseReadOnly, true));
        }
```

3. In the `if let Some(path) = this.input.path.clone().filter(|_| this.input.kind == "file") {` block, directly before its `            items.extend([` (the one starting with `MenuItem::action_with("diff.compareWith"`), add:

```rust
            let text = match &self.content {
                Content::Project { editors, .. } => editors.get(tab).map(|editor| editor.read(cx).text()),
                _ => None,
            };
            items.extend(self.tab_file_rows(&path, in_project, text.as_deref(), window, cx));
```

4. After the `tab_menu` method add:

```rust
    /// The ids of the tabs whose editor shows a read-only document.
    fn read_only_tabs(&self, cx: &App) -> std::collections::HashSet<String> {
        match &self.content {
            Content::Project { editors, .. } => editors
                .iter()
                .filter(|(_, editor)| editor.read(cx).is_read_only())
                .map(|(id, _)| id.clone())
                .collect(),
            _ => Default::default(),
        }
    }
```

5. In `pub fn run_tab_command`, directly before `        self.with_tabs(window, cx, |tabs, _| match command {` add:

```rust
        let read_only = self.read_only_tabs(cx);
```

and in that `match command {` add the arm after `            TabCommand::CloseAllButPinned => tabs.close_all(&pane, true),`:

```rust
            TabCommand::CloseReadOnly => tabs.close_unmodified(&pane, |tab| !read_only.contains(&tab.id)),
```

(`close_unmodified` closes the unpinned tabs its predicate says no to, which here are the read-only ones.)

6. The caller: replace

```rust
                                    let items = workbench.tab_menu(&menu_pane, &menu_tab, cx);
```

with

```rust
                                    let items = workbench.tab_menu(&menu_pane, &menu_tab, window, cx);
```

### 1f. The tool window's Options button and the stripe button's menu

1. In `let tool_window = |id: &str| {`, replace `                    let hidden = id.to_owned();` with:

```rust
                    let (hidden, options) = (id.to_owned(), id.to_owned());
```

and directly before the child `div()` that has `.id(SharedString::from(format!("hide:{id}")))` insert:

```rust
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("more:{id}")))
                                            .debug_selector(|| format!("more:{id}"))
                                            .with_tooltip("Options")
                                            .size(gpui_kit::px(22.0))
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(gpui_kit::px(4.0))
                                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                                            .child(theme::icon(cx, "more", 16.0, theme::color(cx, "fg-muted")))
                                            .on_mouse_down(
                                                gpui_kit::MouseButton::Left,
                                                cx.listener(move |workbench, event: &MouseDownEvent, window, cx| {
                                                    window.prevent_default();
                                                    cx.stop_propagation();
                                                    workbench.show_tool_window_options(
                                                        &options,
                                                        event.position,
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ),
                                    )
```

2. In `let stripe_button = |declared: &ToolWindow| {`, after `                    let action = format!("toolwindow.{}", declared.id);` add `                    let menu_id = declared.id.clone();`, and after its `.on_click(cx.listener(move |_, _, window, cx| { … }))` add:

```rust
                        .on_mouse_down(
                            gpui_kit::MouseButton::Right,
                            cx.listener(move |workbench, event: &MouseDownEvent, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                                workbench.show_stripe_button_menu(&menu_id, event.position, window, cx);
                            }),
                        )
```

### 1g. The Find tool window's match menu

Replace

```rust
                    let opens = cx.subscribe_in(&find, window, |workbench, _, event: &FindPanelEvent, window, cx| {
                        let FindPanelEvent::Open { path, line, col } = event;
                        workbench.open_project_file_at(path.clone(), *line, *col, window, cx);
                    });
```

with

```rust
                    let opens =
                        cx.subscribe_in(&find, window, |workbench, _, event: &FindPanelEvent, window, cx| match event {
                            FindPanelEvent::Open { path, line, col } => {
                                workbench.open_project_file_at(path.clone(), *line, *col, window, cx);
                            }
                            FindPanelEvent::ContextMenu { path, line, col, position } => {
                                workbench.show_search_result_menu(path.clone(), *line, *col, *position, window, cx);
                            }
                        });
```

### 1h. The existing test `a_tabs_menu_closes_pins_and_splits`

The file rows now sit between Split Down and Copy Path/Reference…. Replace its `assert_eq!(rows.iter().map(|(label, _)| label.as_str()).collect::<Vec<_>>(), [ … ]);` (the exact list ending with `"Configure Editor Tabs…",`) with:

```rust
        let shown: Vec<&str> = rows.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(
            shown[..11],
            [
                "Close",
                "Close Other Tabs",
                "Close Tabs to the Left",
                "Close Tabs to the Right",
                "Close All Tabs",
                "Close Unmodified Tabs",
                "Pin Tab",
                "Split and Move Right",
                "Split and Move Down",
                "Split Right",
                "Split Down",
            ]
        );
        // The file's rows: the bookmark group, then Open In, Local History and Git where their actions
        // are registered; then the rows of the file actions that are registered.
        assert!(shown.contains(&"Local History") && shown.contains(&"Git"), "{shown:?}");
        assert_eq!(shown[shown.len() - 3..], ["Copy Path/Reference…", "Rename File…", "Configure Editor Tabs…"]);
```

The later `cx.simulate_keystrokes("down down down enter")` (Close Tabs to the Right) and `"p enter"` (Pin Tab) are unaffected: both rows are above the inserted ones. The test's "Close Tabs to the Right" step opens the menu on tab 1: still the fourth selectable row.

### 1i. `show_menu` split, so a menu built outside the workbench's update can be shown

The editor's root row Column Selection Mode is a toggle whose state reads the workbench (`editor/command_actions.rs` `TOGGLES` → `workbench::active_editor` → `for_window(…).read(cx)`); `PopupMenu::new` resolves it, so building that menu inside the workbench's update would panic. `context_menus.rs` (`show_menu_later`) builds it in `window.defer` and hands it over. Replace the body of `pub fn show_menu<C: Clone + 'static>(` from `        let menu = cx.new(|cx| PopupMenu::new(items, position, window, cx));` to its closing `        self.open_popup(&menu, events, window, cx);` with:

```rust
        let menu = cx.new(|cx| PopupMenu::new(items, position, window, cx));
        self.show_popup_menu(menu, on_command, window, cx);
    }

    /// Shows `menu`, built by the caller, as [`Workbench::show_menu`] shows the menu it builds.
    pub(super) fn show_popup_menu<C: Clone + 'static>(
        &mut self,
        menu: Entity<PopupMenu<C>>,
        on_command: impl Fn(&mut Self, C, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let events = cx.subscribe_in(&menu, window, move |workbench, _, event: &PopupMenuEvent<C>, window, cx| {
            workbench.close_popup(window, cx);
            match event.clone() {
                PopupMenuEvent::Dismissed => {}
                PopupMenuEvent::Chosen(Choice::Command(command)) => on_command(workbench, command, window, cx),
                PopupMenuEvent::Chosen(Choice::Action { id, arg }) => {
                    // Once this update is over: the action may reach for the workbench.
                    window.defer(cx, move |window, cx| {
                        execute_action_from(ActionSource::ContextMenu, &id, arg.as_ref(), window, cx);
                    });
                }
            }
        });
        self.open_popup(&menu, events, window, cx);
```

(the method's closing `    }` stays). The tab menu has no toggle row and keeps `show_menu`.

## 2. `app-gpui/src/workbench/code_insight.rs` (Problems › contributed groups)

In the arm `            ProblemsViewEvent::ShowMenu { position, items } => {`, replace

```rust
                self.show_menu(&items, position, run, window, cx);
```

with

```rust
                let items = self.with_problem_groups(items, window, cx);
                self.show_menu(&items, position, run, window, cx);
```

The problem's place comes from the menu's own Jump to Source row (`ProblemCommand::JumpToSource`), which `problem_menu` always builds for a target.

## 3. `app-gpui/src/workbench/project_files.rs` (Project view › contributed groups)

This supersedes section 6.2 (`tree_context_menu`) of `docs/gpui-port/notes/integration/plugin_contributions_integration.md`; apply its 6.1 (`with_menu_context` takes a `Contributed`) and then this instead of its 6.2. `plugin_project_view_items` stays unused by the window after this; keep it only if the plugin draft's tests call it, else delete it.

1. In `tree_context_menu`, replace (after plugin notes 6.1, the call has the `contributed` argument)

```rust
        let items = cx.update_global::<Localization, _>(|localization, cx| {
            self.with_menu_context(cx, |ctx, _| {
```

with

```rust
        let selected = crate::project_view::context_menu::menu_group_context(&node_refs(&nodes));
        let new_in = selection_target_folder(&self.files.state);
        let contributed = self.project_view_contributed(&selected, new_in.as_deref(), window, cx);
        let items = cx.update_global::<Localization, _>(|localization, cx| {
            self.with_menu_context(contributed, cx, |ctx, _| {
```

2. In `fn show_new_popup`, replace

```rust
        let items = self.with_menu_context(cx, |ctx, _| new_menu_items(ctx, Some(&folder), outside_tree));
```

with

```rust
        let selected = crate::project_view::context_menu::MenuGroupContext::default();
        let contributed = self.project_view_contributed(&selected, Some(&folder), window, cx);
        let items = self.with_menu_context(contributed, cx, |ctx, _| new_menu_items(ctx, Some(&folder), outside_tree));
```

## 4. `app-gpui/src/find/view.rs` (the event the match menu needs)

1. In the `use gpui_kit::{` list add `MouseButton, MouseDownEvent, Pixels, Point,`.
2. Replace

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindPanelEvent {
    /// Open `path` with the caret at the 0-based `line` and UTF-16 column `col`.
    Open { path: PathBuf, line: u32, col: u32 },
}
```

with

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum FindPanelEvent {
    /// Open `path` with the caret at the 0-based `line` and UTF-16 column `col`.
    Open { path: PathBuf, line: u32, col: u32 },
    /// Show the context menu of the match at `path`, 0-based `line`, UTF-16 column `col`, at
    /// `position` of the window.
    ContextMenu { path: String, line: u32, col: u32, position: Point<Pixels> },
}
```

(`Eq` goes: `Pixels` is a float.)

3. After `fn open_selected(&mut self, cx: &mut Context<Self>) { … }` add:

```rust
    /// The right button was pressed on row `index`: a match is selected and its menu asked for.
    fn menu_requested(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(Row::Match(row)) = self.rows.get(index) else { return };
        let (path, line, col) = (row.path.clone(), row.line, row.col);
        self.selected = Some(index);
        cx.emit(FindPanelEvent::ContextMenu { path, line, col, position });
        cx.notify();
    }
```

4. In `render`, after the match row's

```rust
                                    .on_click(cx.listener(move |panel, event: &gpui_kit::ClickEvent, _, cx| {
                                        panel.selected = Some(index);
                                        if event.click_count() >= 2 {
                                            panel.open(index, cx);
                                        }
                                        cx.notify();
                                    }))
```

add

```rust
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(move |panel, event: &MouseDownEvent, _, cx| {
                                            panel.menu_requested(index, event.position, cx);
                                            cx.stop_propagation();
                                        }),
                                    )
```

## 5. `app-gpui/src/workbench/title_bar.rs` (the `mainToolbar` slot)

In `impl Render for TitleBar`, `TitleBarMode::Full`, replace

```rust
                    .children(right)
                    .children(gear)
```

with

```rust
                    .children(right)
                    .children(
                        (self.kind == TitleBarKind::Project)
                            .then(|| super::context_menus::slot_buttons("mainToolbar", window, cx))
                            .unwrap_or_default(),
                    )
                    .children(gear)
```

## 6. `app-gpui/src/vcs/commit_box.rs` (the `commit.message` slot)

1. Replace `    fn render_bar(&self, cx: &mut Context<Self>) -> Div {` with `    fn render_bar(&self, window: &Window, cx: &mut Context<Self>) -> Div {` (add `Window` to the `gpui_kit` import if it is not there).
2. In `render_bar`, after the `.child(icon_button(cx, "history", …) … )` child (the one with `.debug_selector(|| "commit-history".to_owned())`) and before `            .child(div().flex_1())`, add:

```rust
            .children(crate::workbench::context_menus::slot_buttons("commit.message", window, cx))
```

3. In `fn render(&mut self, _: &mut Window, cx: &mut Context<Self>)`, rename `_` to `window` and replace `        let bar = self.render_bar(cx);` with `        let bar = self.render_bar(window, cx);`.

## 7. What `context_menus.rs` holds

- `MENU_GROUPS` (25 `GroupDecl`: id, menus, order, row function or `None`), `GroupFacts`, `native_groups`, `into_menu`.
- Row builders (pure, unit-tested): `editor_menu`, `open_in_menu`, `local_history_menu`, `git_menu`, `gutter_menu`, `tool_window_options`, `stripe_button_menu`, `move_to_menu`, `clipboard_shortcuts`.
- Commands: `EditorMenuCommand` (Cut, Copy, Paste run `edit.cut` / `edit.copy` / `edit.paste` through the registry; CopyReference), `GutterCommand::Toggle`, `ToolWindowCommand`.
- `impl Workbench`: `group_rows`, `open_editor_context_menu`, `editor_menu_items`, `show_gutter_menu`, `tab_file_rows`, `with_problem_groups`, `show_search_result_menu`, `project_view_contributed`, `show_tool_window_options`, `show_stripe_button_menu`.
- `slot_buttons(slot, window, cx)`.
- Tests: 4 `#[test]`s on the rows, 9 end-to-end `#[gpui_kit::test]`s. The literal order of the editor menu is asserted by the `#[test]` `the_editor_menu_lists_its_rows_in_order`; the end-to-end tests compare the shown root with `editor_menu_items` filtered by the registry (so they check the filtering and the opening), plus literal anchors (Paste before Go To, Cut/Copy/Paste adjacent, Git › Commit File… state).

### The groups and their source

| Group                        | Menus (order)                                                | Source                                        | Native                                                                                                                                      |
| ---------------------------- | ------------------------------------------------------------ | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `agents.send`                | editor, projectView (80)                                     | src/features/agents/index.ts:109              | built                                                                                                                                       |
| `build.module`               | projectView (40)                                             | src/features/build/index.ts:125               | built                                                                                                                                       |
| `debugger.evaluate`          | editor                                                       | src/features/debugger/index.ts:73             | built (key `debugSessionPaused`)                                                                                                            |
| `debugger.hotswap`           | editor                                                       | src/features/debugger/index.ts:78             | gap: `hotSwapMenuItems` (classes of the paused JVM session)                                                                                 |
| `debugger.stop`              | stopProcess                                                  | src/features/debugger/index.ts:84             | gap: the Stop chooser is not a menu of this work                                                                                            |
| `deployment`                 | projectView, editor, editorTab (60)                          | src/features/deployment/index.ts:88           | gap: deployment servers (`deployInfo`)                                                                                                      |
| `github.reviewMode`          | editor (60)                                                  | src/features/github/reviewMode/index.ts:146   | gap: review mode state                                                                                                                      |
| `go.modules`                 | projectView, editorTab, editor                               | src/features/run/goActions.ts:103             | gap: Go module menus                                                                                                                        |
| `gradle.analyzeDependencies` | editor, editorTab, projectView (30)                          | src/features/build-tools/index.ts:211         | gap: `analyzerGradle` (build model)                                                                                                         |
| `http.new`                   | new (20)                                                     | src/features/http-client/index.ts:136         | built                                                                                                                                       |
| `http.spring`                | editor (40)                                                  | src/features/http-client/index.ts:126         | gap: Spring endpoints of the file                                                                                                           |
| `maven.analyzeDependencies`  | editor, editorTab, projectView (30)                          | src/features/build-tools/index.ts:224         | gap: Maven model                                                                                                                            |
| `plugins`                    | editor, projectView, editorTab, problems, searchResults (90) | src/features/plugins/uiPoints.tsx:569         | built from `plugin_menu_items`; Open With › (custom editors) is a gap                                                                       |
| `run.context`                | editor, projectView, editorTab                               | src/features/run/index.ts:440                 | gap: `run.runContext`, `run.debugContext`, `run.modifyContext` are not registered; `context_run::context_menu_items` already makes the rows |
| `scratch.new`                | new (0)                                                      | src/features/scratch/index.ts:46              | built                                                                                                                                       |
| `search.structure.selectIn`  | selectIn                                                     | src/features/search/index.ts:194              | built (runs `toolwindow.structure`)                                                                                                         |
| `search.bookmarks`           | editorGutter (20)                                            | src/features/search/index.ts:209              | built                                                                                                                                       |
| `search.bookmarks.files`     | projectView, editorTab (60)                                  | src/features/search/index.ts:226              | built                                                                                                                                       |
| `settings.fileType`          | projectView, editorTab (90)                                  | src/features/settings/fileTypesRuntime.ts:130 | gap: file type overrides                                                                                                                    |
| `templates.emmet`            | editor (60)                                                  | src/features/templates/index.ts:180           | built                                                                                                                                       |
| `templates.new`              | new (10)                                                     | src/features/templates/index.ts:99            | gap: the enabled file templates and their `templates.new.<id>` actions                                                                      |
| `templates.newFromTemplate`  | new (100)                                                    | src/features/templates/index.ts:114           | built                                                                                                                                       |
| `vcs.gutter`                 | editorGutter (10)                                            | src/features/vcs/index.ts:309                 | built; no blame is drawn natively, so the label is always Annotate                                                                          |
| `web.open`                   | editor, editorTab, projectView (5)                           | src/features/web/index.ts:100                 | built                                                                                                                                       |
| `web.openIn`                 | openIn                                                       | src/features/web/index.ts:118                 | gap: the browsers found and the built-in preview                                                                                            |

The two `registerMenuSlotItem` calls (src/features/plugins/uiPoints.tsx:177 `pluginTools`, :182 `commit.message` / `editor.title` / `mainToolbar`): `pluginTools` is the plugin draft's Tools menu slot. The three toolbar slots are icon buttons, shown while the entry's `when` holds and greyed out while its action cannot run. `editor.title` goes in the tab strip (`EditorArea.tsx:478`), `mainToolbar` at the end of the main toolbar (`TitleBar.tsx:361`), `commit.message` above the message (`CommitView.tsx:986`).

### Rows whose action is not registered (left out, as `resolve` does)

Measured: `the_editor_menu_leaves_out_the_actions_the_window_does_not_register` prints the editor menu's list (`cargo nextest run -p openidea_gpui context_menus --no-capture`). Hypothesis, grep-derived (no quoted id anywhere outside keymaps, `vim/rc.rs`, `menu_spec.rs`; ids built at run time would be missed): `navigate.selectIn`, `project.openInTerminal`, `navigate.superMethod`, `navigate.gotoTest`, `code.unfoldAll`, `code.foldAll`, `code.foldSelection`, `refactor.thisMenu`, `code.generate`, `code.pasteFromHistory`, `vcs.showHistoryForSelection`, `emmet.updateTag`, `web.openInBrowser`, `vcs.annotate` (no handler found), `run.runContext` / `run.debugContext` / `run.modifyContext`. These are the gaps other work must fill.

## 8. Behaviours not built

- Keyboard opening of the editor menu: not in the source (see section 0).
- Open in New Window (tab menu) and Exclude / Replace Selected / the other own rows of the Find results menu: no native detached editor window, no exclusion model in `find/view.rs`. Only the `searchResults` groups are shown; no menu opens when they are empty.
- The gutter's Soft-Wrap row: the native editor has no soft wrap. Configure Soft Wraps… and the rest are built.
- A tool window's own header items (`headerMenu` / `titleMenu`) before Move To: no native registry of them.
- Check marks on rows of a menu's own (Move To, Appearance, Show Tool Window Names) are drawn as the `check` icon: `MenuEntry` has no `checked` field.
- Tabs without a path (virtual inputs) get no `editorTab` groups (`src/editor/core/virtual.test.ts` "Pathless tabs still get contributed groups"): `MenuGroupContext` has no `input` field.
- Close All Read-Only Tabs closes through `TabsState::close_unmodified`, as the window's other Close rows do; the webview's `removeGuarded` prompt for a dirty document is not asked (a read-only document is not edited). Read-only is known only for tabs whose editor exists (`editors` map).
- The stripe button's menu and Show Tool Window Names have no test; native stripes show no names, so the setting is written without a visible effect.
- `platform::settings::set` blocks on the main thread (`backend.block_on`). It is the crate's existing way to write a setting (`view.toggleTheme`), not a background task.
- Show Context Actions has no bulb icon: an action row takes its action's icon and `code.showIntentions` has none.
- The Cut/Copy/Paste hints are the keymap's for `edit.cut` / `edit.copy` / `edit.paste`. The presets leave them unbound (`app_models/console_copy.rs` tests), so the OS keys are shown instead (`host_binding("Meta+C")`), as the menu bar's Edit roles and the Problems menu's Copy do.

## 9. Assumptions to check first

1. `MenuItem<Infallible>` mapped through `MenuItem::map(&|never: Infallible| -> C { match never {} })` compiles.
2. `Box::new(move |menu: ContextMenu, ctx: &MenuGroupContext<'_>| Ok(rows(menu, ctx, &facts)))` coerces to `GroupItems<GroupRow>` (higher-ranked over both lifetimes).
3. `ide_core::settings::Settings` serializes with camelCase keys, so `/editor/appearance/lineNumbers`, `/editor/folding/showOutline`, `/editor/breadcrumbs/show`, `/appearance/showToolWindowNames` are found. `editor.showGutterIcons` and `editor.showWhitespace` may not be fields of `EditorSettings`; they then read as off.
4. `I18n::tx` returns an unknown text unchanged, so "Open 'index.html' in Browser" shows as built.
5. A right click in a Markdown file's preview layout also opens the editor menu (the wrapper covers the whole body).
6. The Hello fixture's own manifest has an `editor.context` entry for `word-count`, as the plugin draft's test expects.

## 10. Spec cases

- `src/workbench/ContributedToolbar.test.tsx` (1 case, "renders toolbar slots with command icons and honors when clauses"): partly, by `a_plugins_editor_menu_entries_appear_under_their_submenu_and_run` (the `editor.title` button is drawn); its `when` hiding is not tested.
- `src/editor/core/controller.test.ts` "builds IDEA's editor tab menu": already covered by the existing `a_tabs_menu_closes_pins_and_splits`. "closes read-only tabs and keeps a preview tab open from the tab menu": not covered end to end (a read-only document needs a read-only file in a temp dir and the editor's `is_read_only` to follow it; unverified).
- `src/editor/core/virtual.test.ts` pathless tab groups: not built (section 8).
- No spec file exists for `onContextMenu`, `gutterMenuItems`, `menuGroupItems`, `moveToMenu` (`grep -rln` over `*.test.ts`/`*.test.tsx`).

## 11. Tests most likely to fail first

- `the_tab_menu_acts_on_the_tab_it_was_opened_on` and `move_to_in_a_tool_windows_options_moves_it` need sections 1e and 1f applied.
- `a_plugins_editor_menu_entries_appear_under_their_submenu_and_run` needs the plugin draft compiled and the Hello fixture's wasm (`crates/ide-plugin/tests/fixtures/hello/dist`).
- `go_to_declaration_from_the_editor_menu_moves_the_caret_to_the_declaration` needs `python3` (fake server) and Full mode.
- `the_gutters_menu_adds_a_bookmark_on_the_clicked_line` assumes the gutter is at least 12px wide and that `file.settings` is registered (its two rows are expected).
