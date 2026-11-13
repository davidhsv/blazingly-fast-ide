# Context menus: reconciled integration notes (apply after the three plugin stages)

This file replaces `docs/gpui-port/notes/integration/context_menus_integration.md` for the integration. Apply it after stages 1, 2 and 3 of `docs/gpui-port/notes/integration/plugins_all_integration.md`: `context_menus.rs` calls `plugin_contributions::{PluginMenu, plugin_menu_items, plugin_slot_items}` (stage 3) and its tests call `plugins_window::test_support::{start_with_plugins, plugin_copy, link}` (stage 1). Paths are relative to `app-gpui/src/` unless they start with `app-gpui/` or `docs/`.

**The drafted file.** `workbench/context_menus.rs` as reconciled is in the worktree and, byte-identical (checked with `cmp`), at `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/ctx/context_menus.rs` (1781 lines). A process restored drafted files to their committed content once today: if `git status` shows `app-gpui/src/workbench/context_menus.rs` unmodified, copy it from there first. The committed draft (`24d53646`) is kept at `.../scratchpad/ctx/context_menus.orig.rs` for comparison.

**What was measured in this session and what was read.** Nothing was compiled or run. Measured (commands run, output read): every anchor below was counted with `grep -cF` (or a Python `str.count` for multi-line ones) in the working tree at HEAD `479b4c42`, which includes the compiling agent's uncommitted stage-1 and stage-2 edits (`workbench/mod.rs` now declares `plugins_window`, `welcome_pages`, `plugin_runtime`; none of them touches an anchor here; re-counted last, `NOT ONE: 0`). At that point another process was also editing the undeclared drafts `workbench/editor_core.rs`, `workbench/editor_language.rs` and `editor/core_actions.rs`: once they are declared, the editor menu gains Paste from History…, Folding, Rename…, Generate… and Go To › Test, and the literal lists of section 8 (`ROOT`, `GO_TO` in the tests) must be updated; the action registrations were found by grepping the declared modules (`handle(`, `handle_toggle(`, `ActionMeta::new(`, the tables those loop over, and the constants they use; the undeclared drafts `workbench/{app_actions,editor_core,editor_language,github,project_view_actions,services_actions,terminal_actions}.rs`, `editor/{core_actions,folds,paste_history_view}.rs` were excluded after checking that no `mod` line declares them). Read, not measured: the API conformance of `context_menus.rs` (every call checked against today's signature in `app-gpui/src`, `crates/` and `gpui-pre-0.3.12/src`), and every row list (derived from the registry as read; a run prints the real one: `cargo nextest run -p openidea_gpui context_menus --no-capture`, test `the_editor_menu_leaves_out_the_actions_the_window_does_not_register`).

Sections 1 to 6 are all required: each `pub(super)` method of `context_menus.rs` has its only caller in a hunk, so a partial apply fails `-D warnings` on dead code. Run `cargo fmt` afterwards (a few pasted lines sit at 115 to 120 columns).

No action is registered by this work and no shortcut is bound. Backend calls: the settings write of the gutter's Appearance switches and of Show Tool Window Names (`crate::platform::settings::set`, IPC `settings_set`); every other row runs a registered action.

Hunks: 34 edit sites in 7 files (`workbench/mod.rs` 19, `code_insight.rs` 1, `project_files.rs` 2, `plugin_contributions.rs` 3, `find/view.rs` 4, `title_bar.rs` 1, `vcs/commit_box.rs` 4). 20 apply as the draft wrote them (anchors re-counted today). 1 is made exact (4.1: the draft only said which names to add to the import list). 10 are rewritten: 1c (single-line anchor), 1e.3 (`            items.extend([` alone is not unique), 1f.1 second site and 1f.2 second site (the draft's "directly before the child …" and its `{ … }` placeholder become exact quotes), 1h (the literal row list), 2 (`self.show_menu(&items, position, run, window, cx);` occurs twice in `code_insight.rs`), 3.1 and 3.2 (stage 3.6 replaced the draft's `with_menu_context` signature change by a wrapper, so these call `with_contributed_menu_context`), 5 (`!self.menu_expanded`, as the webview's condition), 6.2 (`.child(div().flex_1())` occurs twice in `commit_box.rs`). 3 are new: 3.3 and the two sites of 3.4 (`plugin_contributions.rs` loses the method 3.1 leaves without a caller, and the import only its tests then use). Every anchor of today's code was counted once in its file by `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/ctx/anchors.py` (run from `app-gpui/src`: `NOT ONE: 0`); 3.1's anchor is stage 3's text, counted once in `plugins_all_integration.md`. The draft's assumption about `MenuItem<Infallible>` and the boxed group closure is resolved in the file (typed `GroupItems<GroupRow>` binding, the pattern of `context_run.rs`'s own test).

---

## 1. `workbench/mod.rs`

### 1a. The module

After the line `mod conflicts;` (1 in the file) add:

```rust
pub mod context_menus;
```

`pub` because `vcs/commit_box.rs` (6) and `title_bar.rs` (5) call `crate::workbench::context_menus::slot_buttons`.

### 1b. The editor's right click (editor and gutter menus)

In `fn pane_element`, directly before (1 in the file)

```rust
        let body: Option<gpui_kit::AnyElement> = match (view, host, editor) {
```

add:

```rust
        // A right click in the editor opens its menu (left of the text: the gutter's).
        let menu_editor = editor.clone();
```

Then replace (1 in the file)

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

Webview, measured in source: CodeMirror starts its own mouse selection only for `event.button == 0` (`node_modules/@codemirror/view/dist/index.js:5006`), and the custom styles (`src/editor/core/cm/multiCaret.ts:28-51`, `columnMode.ts:139`) take button 0 or 1 only, so a right press moves nothing until `onContextMenu` runs (`src/features/editor/` does not exist; the editor is `src/editor/`); the gutter's `contextmenu` listener (`src/editor/core/model/slot.ts:173-180`) does not touch the selection, and `breakpointGutter.ts:247` toggles only on button 0 (button 1 enables/disables). Natively: `editor/view.rs` registers `on_mouse_down` and `on_mouse_up` for `MouseButton::Left` only and its `mouse_move` clears the drag when the left button is not pressed (read today), so a right press neither moves the selection nor reaches `EditorEvent::GutterPressed` (the breakpoint toggle, `workbench/debug.rs`): it bubbles to this wrapper. `open_editor_context_menu` (in `context_menus.rs`) focuses the editor, then left of the text opens the gutter's menu without touching the selection; over the text it collapses the selections to a caret at the press unless the press is inside one of them (`EditorSelection.cursor(pos)` unless `ranges.some(r => r.from <= pos && pos <= r.to)`, `src/editor/core/controller.ts:715-722`), publishes the context and opens the editor's menu.

### 1c. The `editor.title` slot in the tab strip (rewritten: one-line anchor)

Directly before (1 in the file)

```rust
                    .children(layout_actions)
```

add

```rust
                    .children(context_menus::slot_buttons("editor.title", window, cx))
```

(`EditorArea.tsx:478`: `<ContributedToolbar slot="editor.title" />` comes before the layout's tab actions.)

### 1d. `TabCommand::CloseReadOnly`

In `pub enum TabCommand {`, after `    CloseAllButPinned,` (1 in the file) add:

```rust
    CloseReadOnly,
```

### 1e. The tab menu's missing rows

1. Replace the signature line (1 in the file)

```rust
    pub fn tab_menu(&self, pane: &str, tab: &str, cx: &App) -> Vec<MenuItem<TabCommand>> {
```

with

```rust
    pub fn tab_menu(&self, pane: &str, tab: &str, window: &Window, cx: &App) -> Vec<MenuItem<TabCommand>> {
```

2. Replace (1 in the file)

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

3. (rewritten: the anchor is the two lines, 1 in the file) Replace

```rust
            items.extend([
                MenuItem::action_with("diff.compareWith", path.clone().into()).labelled("Compare With…"),
```

with

```rust
            let text = match &self.content {
                Content::Project { editors, .. } => editors.get(tab).map(|editor| editor.read(cx).text()),
                _ => None,
            };
            items.extend(self.tab_file_rows(&path, in_project, text.as_deref(), window, cx));
            items.extend([
                MenuItem::action_with("diff.compareWith", path.clone().into()).labelled("Compare With…"),
```

(`in_project` is the `let` two lines above in that block; `editors: HashMap<String, Entity<Editor>>` keyed by tab id.)

4. After the `tab_menu` method (its last lines are `        items\n    }` right before `   /// Carries out a row of the menu of the tab`tab`of`pane`.`, 1 in the file) add, before that doc line:

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

5. In `pub fn run_tab_command`, directly before (1 in the file)

```rust
        self.with_tabs(window, cx, |tabs, _| match command {
```

add

```rust
        let read_only = self.read_only_tabs(cx);
```

and after (1 in the file)

```rust
            TabCommand::CloseAllButPinned => tabs.close_all(&pane, true),
```

add

```rust
            TabCommand::CloseReadOnly => tabs.close_unmodified(&pane, |tab| !read_only.contains(&tab.id)),
```

(`TabsState::close_unmodified(pane, modified)` closes the unpinned tabs for which `modified` is false: here the read-only ones. The earlier `match command { … _ => {} }` needs nothing.)

6. The caller: replace (1 in the file)

```rust
                                    let items = workbench.tab_menu(&menu_pane, &menu_tab, cx);
```

with

```rust
                                    let items = workbench.tab_menu(&menu_pane, &menu_tab, window, cx);
```

7. (no edit) `tab_menu` has no other caller (`grep -rn 'tab_menu(' app-gpui/src`: `mod.rs` only; `github/window_view.rs` has its own `TabCommand`).

### 1f. The tool window's Options button and the stripe button's menu (rewritten: exact anchors)

1. In `let tool_window = |id: &str| {`, replace (1 in the file)

```rust
                    let hidden = id.to_owned();
```

with

```rust
                    let (hidden, options) = (id.to_owned(), id.to_owned());
```

and replace (1 in the file: the only `.child(` followed by the `hide:{id}` button)

```rust
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("hide:{id}")))
```

with

```rust
                                    .child(div().flex_1())
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
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("hide:{id}")))
```

2. In `let stripe_button = |declared: &ToolWindow| {`, replace (1 in the file)

```rust
                    let action = format!("toolwindow.{}", declared.id);
```

with

```rust
                    let action = format!("toolwindow.{}", declared.id);
                    let menu_id = declared.id.clone();
```

and replace (1 in the file)

```rust
                        .on_click(cx.listener(move |_, _, window, cx| {
                            execute_action_from(ActionSource::Toolbar, &action, None, window, cx);
                        }))
```

with

```rust
                        .on_click(cx.listener(move |_, _, window, cx| {
                            execute_action_from(ActionSource::Toolbar, &action, None, window, cx);
                        }))
                        .on_mouse_down(
                            gpui_kit::MouseButton::Right,
                            cx.listener(move |workbench, event: &MouseDownEvent, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                                workbench.show_stripe_button_menu(&menu_id, event.position, window, cx);
                            }),
                        )
```

(`on_click` fires for the left button only in gpui 0.3.12 `div.rs`, so a right press does not also toggle the window.)

### 1g. The Find tool window's match menu

Replace (1 in the file)

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

This is the only irrefutable `let FindPanelEvent::Open` in the crate (`grep -rn 'FindPanelEvent::'`), so section 4 breaks nothing else.

### 1h. The existing test `a_tabs_menu_closes_pins_and_splits` (rewritten: the literal list)

`search_project`'s `src/alpha.rs` is a file of the project. Its tab menu now has, after Split Down: the `editorTab` groups (`search.bookmarks.files` → Add Bookmark; no plugin host, so no `plugins` rows), Open In (hidden: none of `navigate.selectIn`, `project.revealInFinder`, `project.openInTerminal` is registered and no `openIn` group has rows), Local History (hidden: `vcs.localHistory`, `localHistory.putLabel` unregistered), Git (shown: `vcs.commit`, `vcs.addToVcs`, `vcs.showDiff`, `vcs.rollback`, `vcs.push`, `vcs.update`, `vcs.fetch`, `vcs.branches` are registered), the Compare rows (hidden, as today). Replace (1 in the file)

```rust
                "Split Down",
                // The rows of the file actions that are registered: Copy Absolute Path, Rename File…
                "Copy Path/Reference…",
```

with

```rust
                "Split Down",
                // The file's rows whose actions are registered: the bookmark group, Git, Copy Path/Reference…
                // (Copy Absolute Path) and Rename File…
                "Add Bookmark",
                "Git",
                "Copy Path/Reference…",
```

The rest of the test holds: the disabled rows are still only `Close Tabs to the Left` (Add Bookmark's `bookmarks.toggleFileBookmark` has `when: projectOpen`; a submenu row is never greyed), `"down down down enter"` still reaches Close Tabs to the Right and `"p enter"` still Pin Tab (no other row starts with P).

### 1i. `show_menu` split, so a menu built outside the workbench's update can be shown

Applies as drafted. Replace the body of `pub fn show_menu<C: Clone + 'static>(` from (1 in the file) `        let menu = cx.new(|cx| PopupMenu::new(items, position, window, cx));` to its closing `        self.open_popup(&menu, events, window, cx);` with:

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

(the method's closing `    }` stays). Why: `PopupMenu::new` resolves every row (`ui/popup_menu.rs` `resolve`, recursing into submenus for the emptiness check) and calls `Action::is_toggled`; `editor.toggleColumnSelection` (`editor/command_actions.rs` `TOGGLES`) reads `workbench::active_editor` → `for_window(…).read(cx)`, which panics inside the workbench's own update. `context_menus.rs::show_menu_later` builds the editor and gutter menus in `window.defer` and hands them to `show_popup_menu`.

**Callers of `show_menu` that must change: none.** Its signature is unchanged. A caller would have to change only if it built, inside the workbench's update, a menu holding a toggle whose state reads the workbench. The toggles that do (`handle_toggle` / `.toggled(` in declared files, read today): `editor.toggleColumnSelection`, `editor.toggleInsertState` (`editor/command_actions.rs`), `vcs.toggleAmend` (`vcs_operations.rs`), `ide.toggleMode` (`mod.rs`), `debug.muteBreakpoints` (`debug.rs`); `view.toggle*`, `find.scrollToResults`, `vcs.log.changes.*`, the tab view toggles and `conflicts.rs`'s read globals or the window context only. Measured: `grep -rnE '"(editor\.toggleColumnSelection|editor\.toggleInsertState|vcs\.toggleAmend|ide\.toggleMode|debug\.muteBreakpoints)"' app-gpui/src` outside registrations, keymaps, `menu_spec.rs` (the menu bar, built outside any update) and tests finds them only in the title bar's mode widget (`title_bar.rs`, a tooltip and a `RunAction` event), `code_insight.rs` (`TOGGLE_MODE_ACTION`, run, not put in a menu), the debugger's toolbar buttons (`debugger/window_view.rs`, `ToolbarButton`, not a `MenuItem`) and `debugger/actions.rs`/`driver.rs` (dispatch): no `MenuItem` holds one. The callers of `show_menu` today: `mod.rs` (tab menu; three `ShowMenu` arms), `structure_window.rs`, `database.rs` (3), `bookmarks_window.rs`, `shelf.rs` (2), `agents.rs` (2), `app_actions.rs` (undeclared), `code_insight.rs` (2), `log_actions.rs` (2), `chrome.rs` (3), `project_files.rs`, `services_window.rs` (2), `vcs_operations.rs`, `log_window.rs` (2, including the Git log's right-click commit menu), `run_configs.rs`, `debug.rs` (4), `conflicts.rs`, `todo_window.rs`, `github.rs` (undeclared, 3), `diff_window.rs` (2), `project_view_actions.rs` (undeclared), `plugin_contributions.rs:695` (stage 3: a plugin's gutter actions, plugin commands only). None holds one of the five; the menus this work extends (tab, Problems, Find, Project view) only gain group rows, which are never those five. The editor's Git submenu row `vcs.toggleAmend` is not in it.

## 2. `workbench/code_insight.rs` (Problems › contributed groups; rewritten: `self.show_menu(&items, position, run, window, cx);` occurs twice)

Replace (1 in the file)

```rust
            ProblemsViewEvent::ShowMenu { position, items } => {
                let target = view.clone();
                let run = move |_: &mut Self, command: ProblemCommand, _: &mut Window, cx: &mut Context<Self>| {
                    target.update(cx, |view, cx| view.run_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
```

with

```rust
            ProblemsViewEvent::ShowMenu { position, items } => {
                let target = view.clone();
                let run = move |_: &mut Self, command: ProblemCommand, _: &mut Window, cx: &mut Context<Self>| {
                    target.update(cx, |view, cx| view.run_command(command, cx));
                };
                let items = self.with_problem_groups(items, window, cx);
                self.show_menu(&items, position, run, window, cx);
```

(`problems_view_event` matches `event.clone()`, so `items` is owned.) The problem's place comes from the menu's own Jump to Source row (`ProblemCommand::JumpToSource(ProblemTarget { path, line, col })`, `code_insight/problems.rs` `problem_menu`). The groups follow the inspection rows with no separator before them, as `ProblemsView.tsx:391` appends them.

## 3. `workbench/project_files.rs` and `workbench/plugin_contributions.rs` (rewritten on top of stage 3.6)

### 3.1 The tree's menu

After stage 3.6.2, `tree_context_menu` reads (stage 3.6.2's text, quoted from `plugins_all_integration.md`):

```rust
        let selected = crate::project_view::context_menu::menu_group_context(&node_refs(&nodes));
        let project_view = self.plugin_project_view_items(&selected.paths, window, cx);
        let contributed = Contributed { project_view, ..Contributed::default() };
        let items = cx.update_global::<Localization, _>(|localization, cx| {
            self.with_contributed_menu_context(contributed, cx, |ctx, _| {
```

Replace its two middle lines (1 in the file once stage 3 is applied; `plugin_project_view_items(` occurs nowhere else)

```rust
        let project_view = self.plugin_project_view_items(&selected.paths, window, cx);
        let contributed = Contributed { project_view, ..Contributed::default() };
```

with

```rust
        // The New submenu creates in the folder the menu's own rows target.
        let lead = nodes.first().map(|node| &**node);
        let new_in = crate::project_view::operations::target_folder(&self.files.state, lead);
        let contributed = self.project_view_contributed(&selected, new_in.as_deref(), window, cx);
```

(`context_menu_items` takes its New folder from `target_folder(ctx.state, nodes.first())`; the webview's `contributedNewItems(parent)`, `src/features/project-tree/contextMenu.ts:106`.) The `plugins` group (order 90) now brings the plugin rows that 3.6.2 brought, between Send to Agent (80) and the Open In submenu: stage 3's `the_context_menus_offer_plugin_entries_where_their_when_holds_and_run_them` asserts only that the entry follows Delete…, which holds.

### 3.2 The New popup

Replace (1 in the file today and after stage 3, which leaves this caller on the wrapper)

```rust
        let items = self.with_menu_context(cx, |ctx, _| new_menu_items(ctx, Some(&folder), outside_tree));
```

with

```rust
        let selected = crate::project_view::context_menu::MenuGroupContext::default();
        let contributed = self.project_view_contributed(&selected, Some(&folder), window, cx);
        let items = self.with_contributed_menu_context(contributed, cx, |ctx, _| {
            new_menu_items(ctx, Some(&folder), outside_tree)
        });
```

`with_menu_context` keeps its other caller, the undeclared `project_view_actions.rs`; nothing else of section 3 touches it. Today the `new` groups' actions (`file.newScratchFile`, `http.newRequest`, `templates.newFromTemplate`) are all unregistered, so the popup shows File and Directory as before: `new_popup_model.rs` `row_of` returns `None` for an action row whose id is not in `actions.registered` (read today).

### 3.3 `workbench/plugin_contributions.rs` (stage-3 draft): the method 3.1 leaves without a caller (new; needed by: `-D warnings`)

Delete (1 in the file)

```rust
    // ---------------------------------------------------------------------------------------------
    // Menus

    /// The plugin group of the Project view's context menu for the selected `paths`.
    pub(super) fn plugin_project_view_items<C: Clone>(
        &self,
        paths: &[String],
        window: &Window,
        cx: &App,
    ) -> Vec<MenuItem<C>> {
        plugin_menu_items(PluginMenu::ProjectView, paths, None, &window_context(window, cx), cx)
    }

```

(the separator line before it stays, it ends the gutter section above).

### 3.4 Its imports (new)

`window_context` is then used only by the tests of that file. Replace (1 in the file)

```rust
use crate::platform::context::{ContextKeys, evaluate_when, window_context};
```

with

```rust
use crate::platform::context::{ContextKeys, evaluate_when};
```

and, in its `mod tests`, replace (1 in the file)

```rust
    use crate::platform::context::ContextValue;
```

with

```rust
    use crate::platform::context::{ContextValue, window_context};
```

## 4. `find/view.rs` (the event the match menu needs)

1. (made exact) Replace (1 in the file)

```rust
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render, ScrollStrategy,
    SharedString, Subscription, Task, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
```

with

```rust
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    Pixels, Point, Render, ScrollStrategy, SharedString, Subscription, Task, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};
```

2. Replace (1 in the file)

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

3. After (1 in the file)

```rust
    fn open_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            self.open(index, cx);
        }
    }
```

add

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

4. In `render`, after the match row's (1 in the file)

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

## 5. `workbench/title_bar.rs` (the `mainToolbar` slot; rewritten: `!menu_expanded`)

In `impl Render for TitleBar`, `TitleBarMode::Full`, replace (1 in the file)

```rust
                    .children(right)
                    .children(gear)
```

with

```rust
                    .children(right)
                    .children(if self.kind == TitleBarKind::Project && !self.menu_expanded {
                        super::context_menus::slot_buttons("mainToolbar", window, cx)
                    } else {
                        Vec::new()
                    })
                    .children(gear)
```

(`TitleBar.tsx:361`: `{toolbar && !menuExpanded && <ContributedToolbar slot="mainToolbar" />}`, `toolbar = !minimal && !lightEdit` (`:338`), which is `TitleBarKind::Project`; `render` has `window: &mut Window`.)

## 6. `vcs/commit_box.rs` (the `commit.message` slot; rewritten: unique anchor)

1. Replace (1 in the file) `    fn render_bar(&self, cx: &mut Context<Self>) -> Div {` with `    fn render_bar(&self, window: &Window, cx: &mut Context<Self>) -> Div {` (`Window` is imported).
2. Replace (1 in the file)

```rust
            .child(div().flex_1())
            .children(
                length.map(
```

with

```rust
            .children(crate::workbench::context_menus::slot_buttons("commit.message", window, cx))
            .child(div().flex_1())
            .children(
                length.map(
```

(`CommitView.tsx:986`: `<CommitMessageActions />` between the history menu and the grow.)

3. Replace (1 in the file) `    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {` with `    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {` and (1 in the file) `        let bar = self.render_bar(cx);` with `        let bar = self.render_bar(window, cx);`.

---

## 7. Edits made to `workbench/context_menus.rs` in this reconciliation

They are in the file already; nothing to apply.

1. `Effects::NONE` → `Effects::default()`: `Effects::NONE` is a private associated const of `tool_windows.rs` (compile error from a sibling module); `mod.rs` uses `Effects::default()`.
2. `native_groups`: the boxed closure is bound as `let items: GroupItems<GroupRow> = Box::new(move |menu, ctx: &MenuGroupContext| Ok(rows(menu, ctx, &facts)));` (the expected type gives the closure its higher-ranked signature and `Ok` its error type), as `context_run.rs`'s own test does. `GroupItems` imported.
3. Clipboard hints: the webview shows the literals `⌘X` / `⌘C` / `⌘V` on every system (`controller.ts:733,743,752`; own rows keep their `shortcut` through `resolveMenuItems`'s spread and `Menu.tsx:359` prints `r.shortcut` as is), not the keymap's or the OS's keys. `ClipboardShortcuts`, `clipboard_shortcuts` and `EditorMenuInput::shortcuts` are gone; `CLIPBOARD_HINTS` holds the three literals. Imports `format_binding`, `host_binding` dropped.
4. Gutter: Configure Soft Wraps… and Configure Gutter Icons… are rows of the menu's own (`GutterCommand::Settings(page)`, run as `file.settings` with the page, deferred): as action rows they showed Settings…'s `⌘,`, which the webview's own rows (`gutterMenu.ts:46-49`) do not. `run_gutter_command` takes `window`.
5. `show_gutter_menu` reads the editor with `editor.read(cx)` (no `read_with` on a `Context` in production code elsewhere in the crate).
6. `slot_buttons`: every entry whose `when` holds has a button, clickable whatever its action's state, with a 14 px icon (`ContributedToolbar.tsx`: `IconButton size="small"` with `action`, no `disabled`; `CommitView.tsx:1062` likewise). The draft hid entries whose action was not registered and greyed out disabled ones. `ActionRegistry` and `is_action_enabled` imports dropped (the tests import `ActionRegistry`).
7. Tests: no secret store swap (`ide_host::plugins::{Consent, FileSecretStore}` and `host.set_secret_store(…)` gone); the plugin window starts with `plugins_window::test_support::start_with_plugins` and links a copy made by `plugin_copy(home, "hello", hello_menus)` through `link`; the plain windows start with `workbench::tests::start`; the Git one runs `workbench::tests::init_repo` (the Git tests' isolation: no machine config, `core.hooksPath` inside the repository); only the language-server test has its own start (`start_with_server`: `FakeServer::install` must precede `Backend::init`). The private `settle`, `settle_app`, `git_in`, `copy_dir`, `link_hello` are gone; every wait is `wait_long`. Exact row lists asserted (section 8). New unit test `the_gutter_menu_opens_settings_pages_and_shows_its_switches`; new end-to-end test `renders_toolbar_slots_with_command_icons_and_honors_when_clauses` (the `ContributedToolbar.test.tsx` case); the gutter test also asserts the caret stays and no breakpoint mark appears. Every test that leaves a menu open closes it with Escape.
8. Lines over 120 columns split (`clipboard_row`, `ROOT`, `hello_menus`). Checked by character count; no comment names the old UI, a task or history.

API check (read against today's code): `MenuGroups::{register, items}`, `MenuGroup`, `GroupItems`, `ContextMenu`, `MenuGroupContext` (`run_producers/context_run.rs`); `Contributed`, `Item`, `MenuGroupContext` with `Default` (`project_view/context_menu.rs`); `PluginMenu`, `plugin_menu_items<C: Clone>(menu, &[String], Option<(u32, u32)>, &ContextKeys, &App)`, `plugin_slot_items(&str, &App) -> Vec<SlotItem>` (stage 3); `ToolWindowLayout::{slot_of, anchor_of, maximized, move_window(id, slot, None), reset_placements, toggle_maximize, hide, remove_from_stripe}`, `Workbench::{with_layout, tool_windows, project_root, vcs_state, editor, bookmarks, notify, publish_context, open_menu, label}`; `Editor::{offset_at, position_of, offset_of, rope, selections, select, selection, caret_position, is_read_only, text, path}`; `buffer::line_col`; `has_bookmark`, `path_bookmark_at`, `BookmarkData::{items, paths}`; `ProblemCommand::JumpToSource(ProblemTarget)`; `platform::settings::{effective, set}` (`IdeError::message`); `NotificationRequest::new`, `.body`; `file_manager_name`, `Os::from_platform`; `Keymap(KeymapState)::shortcut_label`; `ContextKeys::{is_truthy, get}`, `evaluate_when`, `window_context -> Rc<ContextKeys>`; `WithTooltip::{with_tooltip, with_tooltip_shortcut}`; `theme::icon(&App, &str, f32, Hsla)`; icons `more`, `check`, `maximize`, `minimize`, `hide`, `cut`, `copy`, `paste`, `plugin` exist in `assets/gen/icons.json`. The settings keys exist with camelCase serde: `editor.appearance.{lineNumbers, indentGuides, methodSeparators}`, `editor.{showGutterIcons, showWhitespace}`, `editor.folding.showOutline`, `editor.breadcrumbs.show`, `appearance.showToolWindowNames` (`crates/ide-core/src/{settings,editor_settings}.rs`; the draft's assumption 3 is resolved).

## 8. The rows each menu shows (actions registered today plus the plugin stages')

A row whose action is not registered is left out (`ui/popup_menu.rs` `resolve`, as `src/ui/Menu.tsx` `resolveMenuItems`); a submenu none of whose rows shows is left out; separators do not double and none ends a level. Registered, among the ids these menus name (declared modules only): `code.showIntentions`, `editor.copyPathReference`, `editor.copyReference`, `code.pasteAsPlainText`, `editor.toggleColumnSelection`, `navigate.declaration`, `navigate.implementation`, `navigate.typeDeclaration`, `find.usages`, `navigate.callHierarchy`, `navigate.typeHierarchy`, `vcs.commit`, `vcs.addToVcs`, `vcs.showDiff`, `vcs.rollback`, `vcs.push`, `vcs.update`, `vcs.fetch`, `vcs.branches`, `agents.sendToAgent`, `build.buildModule`, `navigate.toggleBookmark`, `bookmarks.toggleFileBookmark`, `file.settings`, `project.copyAbsolutePath`, `project.rename`, `toolwindow.hideActive`, `toolwindow.<id>`, `edit.cut`/`copy`/`paste`. The plugin stages add none of these ids (they add `plugins.installFromDisk`, `plugins.reloadAll`, `plugins.log.*`, `view.backgroundTasks`, `debug.pluginStart`, and `plugin.<id>.<command>` per plugin command). Not registered, with the draft that registers it where one exists: `code.pasteFromHistory` (`workbench/editor_core.rs`), `navigate.gotoTest`, `refactor.rename`, `code.generate` (`workbench/editor_language.rs`), `code.fold`, `code.unfold`, `code.foldAll`, `code.unfoldAll`, `code.foldSelection` (`editor/core_actions.rs`), `navigate.selectIn`, `project.openInTerminal`, `project.revealInFinder`, `project.copyFileName`, `project.copyRelativePath` (`workbench/project_view_actions.rs`), and with no draft found: `navigate.superMethod`, `refactor.thisMenu`, `refactor.{move, changeSignature, extractVariable, extractConstant, extractMethod, inline, pullMembersUp, pushMembersDown, safeDelete}`, `vcs.localHistory`, `localHistory.putLabel`, `vcs.annotate`, `vcs.compareWithBranch`, `vcs.showHistory`, `vcs.showHistoryForSelection`, `diff.compareWith`, `diff.compareWithClipboard`, `debug.evaluate` (not in `debug.rs` `DEBUG_ACTIONS`), `emmet.updateTag`, `web.openInBrowser`, `file.newScratchFile`, `http.newRequest`, `templates.newFromTemplate`, `run.runContext`, `run.debugContext`, `run.modifyContext`.

What changed against the draft because the Git slice's actions exist: the Git submenu (editor, editor tab) now shows, with Commit File…, Add, Show Diff, Rollback…, Push…, Pull…, Fetch, Branches…; the tab menu's literal list gains Add Bookmark and Git. What the draft counted as present and is not: Folding (all five actions are in the undeclared `core_actions.rs`), Rename…, Generate…, Go To › Test, Copy / Paste Special › Paste from History…, the Open In and Local History submenus, Compare With… and Compare with Clipboard.

| Menu                                          | Rows shown (root; `-` separator)                                                                                                                                                                                                                                                                                                                                                     | Test                                                                                                                                 |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| Editor, plain file in a project, no selection | Show Context Actions, -, Paste, -, Copy / Paste Special › (Copy Path/Reference…, Copy Reference, Paste as Plain Text), Column Selection Mode, -, Go To › (Declaration or Usages, Implementation(s), Type Declaration, -, Find Usages, Call Hierarchy, Type Hierarchy), -, Send to Agent, -, Git › (Commit File…, Add, -, Show Diff, Rollback…, -, Push…, Pull…, Fetch, -, Branches…) | `right_click_in_a_plain_file_opens_the_editor_menu_and_moves_the_caret`, `in_a_git_repository_the_editor_menus_git_rows_are_enabled` |
| Editor, with a selection                      | Cut ⌘X, Copy ⌘C, Paste ⌘V in place of Paste                                                                                                                                                                                                                                                                                                                                          | `right_click_in_the_selection_keeps_it_and_copy_puts_it_on_the_clipboard`                                                            |
| Editor, Hello linked                          | …, Send to Agent, -, Hello: Word Count of Active File, Hello › (Hello: Insert Date, Hello: Sync greyed), -, Git                                                                                                                                                                                                                                                                      | `a_plugins_editor_menu_entries_appear_under_their_submenu_and_run`                                                                   |
| Editor, virtual document                      | the rows above without Send to Agent (`agents.send` skips virtual URIs) and without Git, then -, Copy Reference                                                                                                                                                                                                                                                                      | `read_only_and_virtual_documents_change_the_clipboard_and_file_rows` (rows as data)                                                  |
| Gutter, no repository                         | Add Bookmark (F3), -, Configure Soft Wraps…, -, Appearance › (7 switches), Configure Gutter Icons…                                                                                                                                                                                                                                                                                   | `the_gutters_menu_adds_a_bookmark_on_the_clicked_line`                                                                               |
| Editor tab, project file                      | Close … Split Down, -, Add Bookmark, -, Git, -, Copy Path/Reference… › (Absolute Path), Rename File…, -, Configure Editor Tabs…                                                                                                                                                                                                                                                      | `a_tabs_menu_closes_pins_and_splits` (1h), `the_tab_menu_acts_on_the_tab_it_was_opened_on`                                           |
| Project view, file                            | the model's rows, then after Delete…: Build Module, -, Add Bookmark, -, Send to Agent, -, the plugin rows, -                                                                                                                                                                                                                                                                         | stage 3's `the_context_menus_offer_plugin_entries_where_their_when_holds_and_run_them` (plugin rows only)                            |
| Problems                                      | the view's rows, then the `problems` group (plugin rows only)                                                                                                                                                                                                                                                                                                                        | none                                                                                                                                 |
| Find results                                  | only the `searchResults` group (plugin rows); no menu while it is empty                                                                                                                                                                                                                                                                                                              | none                                                                                                                                 |
| Tool window Options                           | Move To › (6 areas, own greyed and checked, -, Restore Default Layout), Maximize, -, Hide (⇧⎋)                                                                                                                                                                                                                                                                                       | `move_to_in_a_tool_windows_options_moves_it`                                                                                         |
| Stripe button                                 | Hide, -, Move To ›, -, Show Tool Window Names                                                                                                                                                                                                                                                                                                                                        | none                                                                                                                                 |

## 9. Behaviours not built, and deviations

- Keyboard opening of the editor menu: not in the source (`controller.ts` opens it from the DOM `contextmenu` event only; Shift+F10 is `run.run` in the Windows keymaps).
- A right click on a line that has a breakpoint, in the breakpoint area, opens the breakpoint popover in the webview (`breakpointGutter.ts:294-303`); natively there is no popover view (`debugger/overlays.rs` has the model only), so the gutter's menu opens there.
- A right click in a Markdown file's preview layout opens the editor menu (the wrapper covers the whole body; `Editor` exposes no painted bounds to tell the preview apart).
- Rows that are own rows in the webview but action rows here show the action's shortcut and are greyed out while the action's `when` fails, and are hidden while it is not registered (the webview's own rows always show and run): Add Bookmark in the gutter (`when: projectOpen`; its shortcut F3 the webview shows too), Annotate (hidden: `vcs.annotate` unregistered), the file bookmark row, Select In › Structure (would show ⌘7; the Select In chooser itself is in the undeclared `project_view_actions.rs`).
- An action row cannot override its icon (`ActionItem` has no icon): Show Context Actions has no `bulb`, Open '…' in Browser takes `web.openInBrowser`'s `web` (the same).
- Gutter Soft-Wrap row (the native editor has no soft wrap); the gutter menu's check marks are drawn as the `check` icon (`MenuEntry` has no `checked`).
- Find results' own rows (Exclude, Replace Selected) and the tab menu's Open in New Window: no native exclusion model, no detached editor window.
- Tool windows' own header items before Move To (`headerMenu` / `titleMenu`); the Options menu opens on the press at the pointer, not anchored below the button.
- Tabs without a path get no `editorTab` groups (`MenuGroupContext` has no `input`); the tab menu's file rows are for `kind == "file"` tabs only (as the native menu's other file rows).
- Close All Read-Only Tabs closes without the `removeGuarded` prompt; read-only is known only for tabs with an editor.
- Slot buttons fall back to the `plugin` icon, not the plugin's own icon (`pluginIconName`).
- `platform::settings::set` blocks on the main thread (`backend.block_on`), the crate's existing way to write a setting.
- New strings that need `en.json` / `zh.json` entries (off limits here): "Close All Read-Only Tabs", "Options" (likely present), "Cannot change the setting".

## 10. Assumptions to check, in order

1. Stage 3 applied as its notes say: `with_contributed_menu_context` exists and 3.6.2's two lines are as quoted (3.1 anchors on them); `plugins_window::test_support` is reachable from `workbench::context_menus::tests` (the same path stage 3's tests use).
2. `crate::workbench::tests::{start, wait_long, init_repo}` (`pub(super)` in a private module) reachable from `workbench::context_menus::tests` (descendant of `workbench`).
3. A right press over the editor reaches the wrapper's `on_mouse_down(MouseButton::Right, …)` and `cx.stop_propagation()` keeps it from the pane's other listeners; GPUI's `track_focus` does not move the selection.
4. `window.defer` in `show_menu_later` runs before `cx.run_until_parked()` returns in the tests (the menu is open right after `right_click`).
5. The registry derivation in section 8: no declared module registers one of the "not registered" ids through a constant or a table that the grep missed (checked by hand for every constant `handle(` call; `the_editor_menu_leaves_out_the_actions_the_window_does_not_register` prints the list).
6. `I18n::tx` returns an unknown text unchanged ("Open 'index.html' in Browser", "Add Bookmark", "Close All Read-Only Tabs").
7. The hello fixture's commands are actions while the plugin is Inactive and their titles are the manifest's (`Hello: Word Count of Active File`), and the editor's `insert-date` runs on the focused editor when its toolbar button is clicked.
8. `workspaceContains:MODULE.bazel` turns true after the file is written into the open project (stage 3's test relies on it too).
9. Settled by reading, kept for the record: the gutter is at least 12 px wide (`debug.rs`'s `clicking_the_gutter_toggles_the_lines_breakpoint` presses 12 px left of the text and passes today); a freshly opened project file's tab is not a preview tab and the Compare rows are hidden (the current literal list of `a_tabs_menu_closes_pins_and_splits` has neither Keep Tab Open nor Compare With…); the handlers take what the rows pass (`navigate.toggleBookmark` reads `{path, line}`, `bookmarks.toggleFileBookmark` `{targets: [{path, dir}]}`, `vcs.showDiff` / `vcs.addToVcs` / `vcs.rollback` an array through `vcs_ops::actions::arg_paths`, `build.buildModule` a string, `agents.sendToAgent` an array or nothing; read in `mod.rs`, `diff_window.rs`, `vcs_operations.rs`, `build_window.rs`, `agents.rs`).

## 11. Compile risks

- 1b's `cx.listener` inside `pane_element(&self, …, cx: &mut Context<Self>)`, with `editor` moved into the closure: `Entity<Editor>` is `Clone`, the listener is `Fn` (`&editor` in the body).
- 1f's closures: `tool_window` and `stripe_button` are called more than once (`.and_then(tool_window)`, `.map(&stripe_button)`), so each must stay `Fn`: the ids are cloned per call and moved into the listeners.
- 3.1: `nodes.first().map(|node| &**node)` gives `Option<&ProjectNode>` from `Vec<Arc<ProjectNode>>`; `target_folder` is `pub` in `project_view/operations.rs` but not imported in `project_files.rs` (hence the full path).
- `GroupRows` is a `fn` pointer type with an elided lifetime inside `MenuGroupContext<'_>`; the `Some(agents_send)` items of the `const MENU_GROUPS` coerce from fn items.
- `let [cut, copy, paste] = CLIPBOARD_HINTS;` (array pattern on a `const`).
- `run_gutter_command`'s `match` arms both evaluate to `()` (`window.defer` returns `()`).
- `the_gutter_menu_opens_settings_pages_and_shows_its_switches` compares `Option<&'static str>` with `Some("check")`.

## 12. Tests most likely to fail first, and why

1. `the_tab_menu_acts_on_the_tab_it_was_opened_on` and `a_tabs_menu_closes_pins_and_splits`: their literal lists depend on assumption 5 (a registered `vcs.localHistory` or `diff.compareWith` would add Local History or the Compare rows).
2. `right_click_in_a_plain_file_opens_the_editor_menu_and_moves_the_caret` and `in_a_git_repository_the_editor_menus_git_rows_are_enabled`: `ROOT`, `GO_TO` and `GIT` are literal; any of the drafts listed in section 8 landing first changes them (Folding, Rename…, Generate…, Test, Open In, Local History, Compare). Update the constants then.
3. `a_plugins_editor_menu_entries_appear_under_their_submenu_and_run` and `renders_toolbar_slots_with_command_icons_and_honors_when_clauses`: the hello fixture's wasm (`crates/ide-plugin/tests/fixtures/hello/dist`), assumptions 7 and 8, and the host's 250 ms flush of command changes.
4. `go_to_declaration_from_the_editor_menu_moves_the_caret_to_the_declaration`: needs `python3` (fake server) and Full mode.
5. `the_gutters_menu_adds_a_bookmark_on_the_clicked_line`: assumption 9.
6. `move_to_in_a_tool_windows_options_moves_it`: the project tool window must be shown in the default layout for `more:project` to be drawn.

## 13. Anchors in regions the compiling agent is likely to touch

It is applying the plugin stages in this worktree now (stage 1's edits to `workbench/mod.rs`, `settings_window.rs`, `welcome.rs`, `plugins_window.rs` were uncommitted at HEAD `479b4c42`). Re-check with `grep -cF` right before pasting: in `workbench/mod.rs` the `Render` regions of `pane_element` (1b, 1c, the tab strip's caller 1e.6) and of the tool windows (1f), the `FIND_TOOL_WINDOW` arm of the panel mounting (1g: the compiling agent's area includes `find/`), `show_menu` (1i), the test `a_tabs_menu_closes_pins_and_splits` (1h, inside `mod tests`); `find/view.rs` (4: the `find/` area); `workbench/code_insight.rs` (2, changed in `e22ffae7` today); `workbench/project_files.rs` (3.1 and 3.2: stage 3.6 rewrites `with_menu_context`; 3.1 anchors on stage 3's own text); `workbench/plugin_contributions.rs` (3.3, 3.4: the stage-3 draft the compiling agent will compile and may fix; re-count the deleted block and both import lines after stage 3). `title_bar.rs`, `vcs/commit_box.rs` and the `TabCommand` enum are outside its stated area.

## 14. Inventory rows the tests would prove (`docs/gpui-port/parity.json` keys)

| Key                                                                                                                                                                                                                                                                  | Test                                                                                                                                                                                                                                                                                  | Status                                                                                                                                                                                                                                               |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `point.registerMenuGroup:agents.send`                                                                                                                                                                                                                                | `right_click_in_a_plain_file_opens_the_editor_menu_and_moves_the_caret` (row shown)                                                                                                                                                                                                   | partial: not run from the menu; the Project view row (paths as argument) has no test                                                                                                                                                                 |
| `point.registerMenuGroup:plugins`                                                                                                                                                                                                                                    | `a_plugins_editor_menu_entries_appear_under_their_submenu_and_run`; stage 3's project view test                                                                                                                                                                                       | partial: editor tab, Problems and Search Results rows untested; Open With › (custom editors) not built                                                                                                                                               |
| `point.registerMenuGroup:search.bookmarks`                                                                                                                                                                                                                           | `the_gutters_menu_adds_a_bookmark_on_the_clicked_line`                                                                                                                                                                                                                                | partial: an action row, so greyed out without a project where the webview's own row is not                                                                                                                                                           |
| `point.registerMenuGroup:search.bookmarks.files`                                                                                                                                                                                                                     | `the_tab_menu_acts_on_the_tab_it_was_opened_on`, `a_tabs_menu_closes_pins_and_splits` (row shown)                                                                                                                                                                                     | partial: not run from the menu                                                                                                                                                                                                                       |
| `point.registerMenuGroup:build.module`                                                                                                                                                                                                                               | none (row built; shown in the Project view)                                                                                                                                                                                                                                           | missing: no test                                                                                                                                                                                                                                     |
| `point.registerMenuGroup:debugger.evaluate`, `:templates.emmet`, `:web.open`, `:vcs.gutter`, `:http.new`, `:scratch.new`, `:templates.newFromTemplate`, `:search.structure.selectIn`                                                                                 | `the_groups_of_a_menu_come_in_order_each_followed_by_a_separator` (rows as data only)                                                                                                                                                                                                 | missing: their actions (`debug.evaluate`, `emmet.updateTag`, `web.openInBrowser`, `vcs.annotate`, `http.newRequest`, `file.newScratchFile`, `templates.newFromTemplate`) are not registered, so the rows never show; Select In has no native chooser |
| `point.registerMenuGroup:debugger.hotswap`, `:debugger.stop`, `:deployment`, `:github.reviewMode`, `:go.modules`, `:gradle.analyzeDependencies`, `:http.spring`, `:maven.analyzeDependencies`, `:run.context`, `:settings.fileType`, `:templates.new`, `:web.openIn` | `every_menu_group_is_listed_once` (listed as missing)                                                                                                                                                                                                                                 | missing: the feature state is not in the window                                                                                                                                                                                                      |
| `point.registerMenuSlotItem:(at) src/features/plugins/uiPoints.tsx:182`                                                                                                                                                                                              | `renders_toolbar_slots_with_command_icons_and_honors_when_clauses`, `a_plugins_editor_menu_entries_appear_under_their_submenu_and_run`                                                                                                                                                | partial: `editor.title` proven; `mainToolbar` and `commit.message` built, untested; the plugin's own icon not used as fallback                                                                                                                       |
| `unit:src/workbench/ContributedToolbar.test.tsx`                                                                                                                                                                                                                     | `renders_toolbar_slots_with_command_icons_and_honors_when_clauses`                                                                                                                                                                                                                    | done: 1 of 1 case (icon asserted only as drawn)                                                                                                                                                                                                      |
| `module:src/workbench/ContributedToolbar.tsx`                                                                                                                                                                                                                        | same                                                                                                                                                                                                                                                                                  | partial: the plugin icon fallback                                                                                                                                                                                                                    |
| `module:src/editor/core/gutterMenu.ts`                                                                                                                                                                                                                               | `the_gutters_menu_adds_a_bookmark_on_the_clicked_line`, `the_gutter_menu_opens_settings_pages_and_shows_its_switches`                                                                                                                                                                 | partial: no Soft-Wrap row; the Appearance switches' writes untested                                                                                                                                                                                  |
| `module:src/editor/core/controller.ts` (existing row, partial)                                                                                                                                                                                                       | adds `right_click_in_a_plain_file_opens_the_editor_menu_and_moves_the_caret`, `right_click_in_the_selection_keeps_it_and_copy_puts_it_on_the_clipboard`, `go_to_declaration_from_the_editor_menu_moves_the_caret_to_the_declaration`, `the_tab_menu_acts_on_the_tab_it_was_opened_on` | partial: the editor menu and the tab menu's file rows; Close All Read-Only Tabs and Open in New Window untested/not built                                                                                                                            |
| `module:src/workbench/toolwindows/ToolWindows.tsx` (existing row, partial)                                                                                                                                                                                           | adds `move_to_in_a_tool_windows_options_moves_it`                                                                                                                                                                                                                                     | partial: the stripe button menu and Show Tool Window Names untested; header menus not built                                                                                                                                                          |
| `module:src/ui/Menu.tsx` (existing row, partial)                                                                                                                                                                                                                     | unchanged by this work                                                                                                                                                                                                                                                                | partial (check marks of own rows)                                                                                                                                                                                                                    |
| `action:edit.copy` (existing row)                                                                                                                                                                                                                                    | `right_click_in_the_selection_keeps_it_and_copy_puts_it_on_the_clipboard` runs it from the menu                                                                                                                                                                                       | as recorded today                                                                                                                                                                                                                                    |
| `ipc:settings_set` (existing row)                                                                                                                                                                                                                                    | none of these tests writes a setting                                                                                                                                                                                                                                                  | unchanged                                                                                                                                                                                                                                            |
