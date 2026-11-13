# Project view, Terminal and Services actions: reconciled integration notes (apply after plugins, context menus, editor, app actions)

This file replaces `docs/gpui-port/notes/integration/project_services_terminal_integration.md` for the integration. Apply it after, in this order: the three plugin stages (`plugins_all_integration.md`), the context menus (`context_menus_reconciled.md`), the editor stages (`editor_all_integration.md`) and the application actions (`app_actions_reconciled.md`, which since commit `a14d240d` replaces `app_actions_integration.md`; none of its hunks touches an anchor here, and its trust question is not met by these tests: they open projects with `Workbench::open_project` on folders trusted with `trust.decide`, its §10.2 seam). Build and run this file's tests once all of it is pasted: it is one stage.

**Measured in this session (commands run, output read).** Every anchor below was counted by `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/pst/anchors.py` (run from the worktree root with `python3 -I`; last run printed `NOT ONE: 0`) at HEAD `a14d240d` (and before it at `150e68e0`), with the compiling agent's uncommitted plugin stage 1–3 edits in the tree (`git status` listed `workbench/{mod,debug,menu,plugin_contributions,plugin_runtime,plugins_window,project_files,settings_window,tab_views,welcome,chrome,app_actions}.rs`, `app-gpui/Cargo.toml`, `debugger/{actions,driver}.rs` and others as modified; `project_files.rs` already carries stage 3.6's `with_contributed_menu_context`). The anchor a pending note creates (H) was counted in that note. Also measured: `panel.rs`, `project_view/*`, `terminal/store.rs` are unchanged since the draft commit `66f27993` (`git log` on them ends at `74c29a58` / `e22ffae7`; `e22ffae7` touched only `project_tree.rs` `selected_directory` among the files these drafts call); the generated `assets/gen/actions.json` / `keymaps.json` entries quoted in §11; that no file outside the three drafts registers any id this file registers (grep of `registry.handle("<id>"`, `ActionMeta::new(`, the tables they loop over); no `impl Workbench` method the drafts define exists elsewhere (grep, E0592 check).

**Read, not measured.** Nothing was compiled or run. API conformance of the drafts (each call read against today's signature in `app-gpui/src`, `crates/`, `gpui-pre-0.3.12/src`), the row lists of §7, and what the tests will do. The assumptions that need a build or a run are in §13, in the order to check them.

Hunks: **31 edit sites in 7 files** (`workbench/mod.rs` 6, `workbench/project_files.rs` 9, `terminal/panel.rs` 6, `editor/actions.rs` 1, `crates/ide-host/src/lib.rs` 4, `crates/ide-host/src/ops/fs.rs` 2, `workbench/context_menus.rs` 3). Of the draft's 27: 13 apply as drafted (re-counted today), 5 are rewritten (A1 also re-exports the Undo entry point; A4 moves off `project_files::register_actions(registry);`, which `editor_all` 1.3 also inserts after; C1 and C2 are the exact, formatted import lists; C6's `layout_element` was past 120 columns), and the draft's §1c (9 hunks making `mod tests` helpers `pub(super)`) is dropped: the tests now use what is already reachable (`workbench::tests::{start, wait_long, run_action, click_on, notification_titles}`, `plugins_window::test_support::{trusted_project, program}`) and small local helpers, so nothing is pasted into `mod tests` except H. 13 are new: A5 (the `terminal.closeSession` handler moves out of `mod.rs`), B9 (the file-manager seam), D (Undo in the Project view), E1–E4 and F1–F2 (the delete seam in the backend and its unit test), G1–G3 and H (the literal row lists that change).

---

## 0. The drafted files

Undeclared, edited in this session; byte-identical copies are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/pst/files/app-gpui/src/workbench/` (checked with `cmp`). **If `git status` shows a drafted file unmodified (or `git diff --stat` shows it as in commit `66f27993`), copy it from there first.**

- `app-gpui/src/workbench/project_view_actions.rs` (1158 lines; 14 end-to-end tests).
- `app-gpui/src/workbench/terminal_actions.rs` (530 lines; 5 end-to-end tests, 1 unit test).
- `app-gpui/src/workbench/services_actions.rs` (126 lines; tests only, 2 end-to-end tests).

Edits made to the drafts in this session:

- **project_view_actions.rs**
  - `EXPAND_LIMIT` (a copy of 400) removed; `project_view::store::EXPAND_ALL_LIMIT` is used.
  - New: `project.revealInFinder` registered with the meta `project_actions` computes (the manifest entry has no title: the title is the file manager's name), handler `reveal_in_finder` (the paths given, else the selection while the tree has the keyboard, else the edited file: `context_target`, as `actions.ts:391`).
  - New: `FileManager` (a `Global` holding the function that shows a path in the OS file manager) and `reveal_in_file_manager(paths, cx)`; B9 sends `Effect::RevealInFileManager` (Open In › Finder, File Path rows, Select In › Finder) through it. Without the global it is `cx.reveal_path`, as today.
  - New: `pub fn undo_in_project_view(window, cx) -> bool` (false when the view has nothing to revert) and `Workbench::undo_file_operation` (Edit › Undo while the tree has the keyboard: `undo_delete` for the last delete, else `project.undoMove`; `undoLastFileOperation`, `src/features/project-tree/operations.ts:509`, wired in `index.ts:209`).
  - `register_view_options` renamed `register_computed` (it also registers Reveal).
  - Two `let _ = this.update…` in detached tasks now log the closed window instead of dropping the error.
  - Tests: `settle` → `wait_long` everywhere; `trusted_project` is `test_support`'s (label `"w1"`, activates the window); local `text_of`, `project_tree_of`, `tree_labels`, `select_in_tree`, `close_project`; **the fixed `std::thread::sleep` loop is gone** (the "nothing copied without a source root" case is the model test `uses_effective_nested_source_test_and_resource_roots_and_respects_exclusions`, `platform/misc.rs`); new helpers `actions_run_by` (records what the keyboard runs, through `ActionRegistry::on_action_executed`) and `record_reveals` (installs a recording `FileManager`); every key test asserts the action ran where its condition holds and did not run where it does not (§11).
  - New tests: `the_navigation_choosers_keys_run_nothing_without_a_project`, `reveal_in_finder_shows_the_selection_or_the_edited_file_in_the_file_manager`, `a_confirmed_delete_moves_the_file_to_the_trash_and_undo_brings_it_back`; File Path now also chooses a row (recorded reveal), Compare With presses ⌘D in the tree and in the editor, Jump to Source presses F4 in the editor, Select In presses ⌥F1 and asserts the literal chooser rows.
- **terminal_actions.rs**
  - New: `terminal.closeSession` (Terminate / Cancel before ending a running command; `close_terminal_session`, `end_terminal_tab`, `close_terminal_tab`), A5 removes the `mod.rs` handler it replaces.
  - `open_terminal_tab`: `fresh.then(|| …).flatten()` → `active_tab().filter(|_| fresh)`.
  - Tests: every terminal starts **`plainsh`**, a `/bin/sh` script run non-interactively, not as a login shell (`plain_shells`: Settings › Terminal › Shell path + Shell integration off, on top of `start`'s `isolate_shells`); New Predefined Session runs `plainsh` instead of `/bin/sh` (which `resolve_shell` would start with `-l`); `close_project`; new test `close_session_asks_before_terminating_a_running_command_and_closes_an_idle_shell_at_once`.
- **services_actions.rs**
  - Its own `services_project` fixture (the private `tests::run_project` / `tests::program` are not reachable): writes `docker.json` with no connection into the temp data folder before the window opens, so showing Services makes no Docker call; asserts no `docker:` row is listed. `close_project`.

## 1. `app-gpui/src/workbench/mod.rs`

### A1. Module and re-export (rewritten: adds the `pub use`)

After `mod project_files;` (1 in the file; plugin stage 2.2.1 put `mod plugin_runtime;` before it) add:

```rust
mod project_view_actions;
pub use project_view_actions::undo_in_project_view;
```

(`pub use` follows the file's `mod settings_window;` / `pub use settings_window::apply_keymap_settings;`; `editor/actions.rs` calls it as `workbench::undo_in_project_view`, D.)

### A2. (as drafted)

After `mod services_window;` (1) add:

```rust
mod services_actions;
```

### A3. (as drafted)

Before `mod todo_window;` (1) add:

```rust
mod terminal_actions;
```

### A4. The registrations (rewritten anchor)

In `pub fn register_actions(cx: &mut App)`, after `    todo_window::register_actions(registry);` (1) add:

```rust
    project_view_actions::register_actions(registry);
    terminal_actions::register_actions(registry);
```

(Not after `project_files::register_actions(registry);`: `editor_all_integration.md` 1.3 inserts `editor_core::register_actions(registry);` there.) Order matters for one id only: A5.

### A5. Close Terminal Session moves to `terminal_actions.rs` (new)

The inline handlers of `register_actions` run after the per-module calls and `register` replaces silently, so this one would win over `terminal_actions.rs`'s. Delete (1 in the file):

```rust
    registry.handle("terminal.closeSession", |_, window, cx| {
        on_terminals(window, cx, |terminals, cx| terminals.close_active_tab(cx))
    });
```

`on_terminals` stays used (`terminal.new`, `terminal.clear`); `TerminalPanel::close_active_tab` stays (`pub`, used by `panel.rs`' tests).

## 2. `app-gpui/src/workbench/project_files.rs`

### B1–B7. Visibility for the sibling module (as drafted)

Prefix with `pub(super) ` (each line is 1 in the file, also after stage 3.6, whose replacement keeps `fn with_menu_context<R>(…` as its first line):

- `type Done = Box<dyn FnOnce(&mut Workbench, Outcome, &mut Window, &mut Context<Workbench>)>;` (in `start_flow`'s signature)
- `    fn with_view<R>(`
- `    fn start_flow(`
- `    fn nodes_for(&mut self, arg: Option<&Value>, cx: &mut Context<Self>) -> Vec<Arc<ProjectNode>> {`
- `    fn active_file_path(&self, cx: &App) -> Option<String> {`
- `    fn with_menu_context<R>(&self, cx: &App, build: impl FnOnce(&MenuContext<'_>, &App) -> R) -> R {`
- `    fn run_project_command(`

### B8. Mark Directory as is carried out (as drafted)

In `fn carry_out_effect`, replace (1)

```rust
            Effect::MarkDirectory { path, .. } => {
                let request = NotificationRequest {
                    body: Some(path),
                    ..NotificationRequest::new(NotificationLevel::Warning, "Mark Directory as is not available here")
                };
                self.notify(request, cx);
            }
```

with

```rust
            Effect::MarkDirectory { path, role } => self.mark_directory_as(path, role, window, cx),
```

`NotificationRequest` / `NotificationLevel` stay used (`run_flow_action`).

### B9. The file manager through the seam (new)

In `fn carry_out_effect`, replace (1)

```rust
            Effect::RevealInFileManager(paths) => {
                for path in paths {
                    cx.reveal_path(Path::new(&path));
                }
            }
```

with

```rust
            Effect::RevealInFileManager(paths) => super::project_view_actions::reveal_in_file_manager(paths, cx),
```

`Path` stays used (`BackendCall::FsCreateDir`).

## 3. `app-gpui/src/terminal/panel.rs` (split panes; checked against today's file)

`panel.rs` is unchanged since the draft (`git log -- app-gpui/src/terminal/panel.rs` ends at `74c29a58`), and every API the hunks call exists as the draft read it: `Terminals::{layout, selected, tab, active_tab, split_tab, unsplit, new_tab, new_tab_at, close_tab}`, `GroupLayout::{Group, Split}`, `GroupSplit { orientation, children }`, `GroupLayout::groups() -> Vec<&str>`, `SplitOrientation::{Horizontal, Vertical}`, `theme::color(cx: &App, …)` (token `border` used by `everywhere/bookmarks_window.rs`), `Div::debug_selector(impl FnOnce() -> String)`.

### C1. Imports (rewritten: exact, formatted)

Replace (1)

```rust
use super::store::{SessionCommand, SessionSnapshot, SessionStatus, TerminalEffect, Terminals, start_directory};
```

with

```rust
use super::store::{
    GroupLayout, SessionCommand, SessionSnapshot, SessionStatus, SplitOrientation, TerminalEffect, Terminals,
    start_directory,
};
```

### C2. (rewritten: exact, formatted)

Replace (1)

```rust
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString, Subscription,
    Window, div, prelude::*, px,
};
```

with

```rust
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
```

### C3. The accessors and the mutator (as drafted)

After (1)

```rust
    pub fn terminals(&self) -> &Terminals {
        &self.terminals
    }
```

add

```rust
    /// The project folder terminals start in.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Carries out `change` on the tabs, then what it asked for: the shells of the tabs that show
    /// start and the project's tabs are stored. The tab the panel started with counts as claimed.
    pub fn change<R>(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Terminals) -> R) -> R {
        self.fresh = false;
        let result = change(&mut self.terminals);
        self.carry_out(cx);
        result
    }

    /// The terminal of tab `id`, once its shell has started.
    pub fn view_of_tab(&self, id: &str) -> Option<&Entity<TerminalView>> {
        let tab = self.terminals.tab(id)?;
        self.views.get(&tab.session).map(|(view, _)| view)
    }

    /// The tab groups the window shows, in layout order, each with its selected tab.
    pub fn panes(&self) -> Vec<(String, Option<String>)> {
        let selected = self.terminals.selected();
        let groups = self.terminals.layout().groups().into_iter();
        groups.map(|group| (group.to_owned(), selected.get(group).cloned())).collect()
    }
```

### C4. Every pane's shell starts (as drafted; exact quote)

Replace (1, the whole function)

```rust
    /// Starts the shell of the active tab's session if it has none yet. A tab that is not
    /// showing, such as one remembered from the last run, costs no process until it is selected.
    fn start_active(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.terminals.active_session().map(|session| session.key.clone()) else { return };
        if self.views.contains_key(&key) {
            return;
        }
        let Some(options) = self.options(&key, cx) else { return };
        let label = self.window_label.clone();
        let view = cx.new(|cx| TerminalView::new(&label, options, cx));
        let session = key.clone();
        let events = cx.subscribe(&view, move |panel: &mut TerminalPanel, view, event: &TerminalEvent, cx| {
            panel.terminal_event(&session, &view, event, cx);
        });
        let state = self.snapshot(&key, &view, cx);
        self.views.insert(key.clone(), (view, events));
        self.terminals.set_session_state(&key, state);
    }
```

with

```rust
    /// Starts the shells of the tabs that show, one per tab group, that have none yet. A tab that is
    /// not showing, such as one remembered from the last run, costs no process until it is selected.
    fn start_active(&mut self, cx: &mut Context<Self>) {
        let shown: Vec<String> = self
            .terminals
            .layout()
            .groups()
            .into_iter()
            .filter_map(|group| self.terminals.selected().get(group))
            .filter_map(|id| self.terminals.tab(id))
            .map(|tab| tab.session.clone())
            .collect();
        for key in shown {
            self.start_session(&key, cx);
        }
    }

    /// Starts the shell of session `key` if it has none yet.
    fn start_session(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.views.contains_key(key) {
            return;
        }
        let Some(options) = self.options(key, cx) else { return };
        let label = self.window_label.clone();
        let view = cx.new(|cx| TerminalView::new(&label, options, cx));
        let session = key.to_owned();
        let events = cx.subscribe(&view, move |panel: &mut TerminalPanel, view, event: &TerminalEvent, cx| {
            panel.terminal_event(&session, &view, event, cx);
        });
        let state = self.snapshot(key, &view, cx);
        self.views.insert(key.to_owned(), (view, events));
        self.terminals.set_session_state(key, state);
    }
```

With one group this starts exactly what the old code started (the group's selected tab is the active tab), so `remembered_tabs_start_their_shells_when_first_shown` (1 started, then 2) and `tabs_are_shells_of_their_own` keep their expectations.

### C5. The groups are drawn (as drafted)

In `fn render`, replace (1)

```rust
            .child(div().flex_1().min_h_0().children(self.active_view().cloned()))
```

with

```rust
            .child(div().flex_1().min_h_0().flex().child(self.layout_element(self.terminals.layout(), cx)))
```

### C6. (rewritten: was past 120 columns)

After (1)

```rust
    pub fn started(&self) -> usize {
        self.views.len()
    }
```

add

```rust

    /// The tab groups as the layout splits them, side by side or stacked, each showing its
    /// selected tab's terminal, with a one-pixel line between them.
    fn layout_element(&self, layout: &GroupLayout, cx: &App) -> AnyElement {
        match layout {
            GroupLayout::Group(group) => {
                let view = self.terminals.selected().get(group).and_then(|id| self.view_of_tab(id)).cloned();
                let selector = format!("terminal-group:{group}");
                div()
                    .debug_selector(move || selector.clone())
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .size_full()
                    .children(view)
                    .into_any_element()
            }
            GroupLayout::Split(split) => {
                let pane = div().flex_1().min_w_0().min_h_0().size_full().flex().gap(px(1.0));
                let pane = pane.bg(theme::color(cx, "border"));
                let pane = match split.orientation {
                    SplitOrientation::Horizontal => pane.flex_row(),
                    SplitOrientation::Vertical => pane.flex_col(),
                };
                pane.children(split.children.iter().map(|child| self.layout_element(child, cx))).into_any_element()
            }
        }
    }
```

`render` takes `cx: &mut Context<Self>`, passed as `&App` by deref coercion. `self.terminals.layout()` and `self.layout_element` both borrow `self` shared.

## 4. `app-gpui/src/editor/actions.rs` (D, new: Undo in the Project view)

`editor_all_integration.md` stage 2 (2.7) replaces the `edit.undo` registration; its replacement keeps the first line `    registry.handle("edit.undo", |_, window, cx| {` (1 today, and 1 in its replacement text: quoted from that note:

```rust
    registry.handle("edit.undo", |_, window, cx| {
        // With nothing of its own left to undo, the editor undoes the refactoring of its file.
        if workbench::undo_refactoring(window, cx) {
            return Ok(());
        }
```

). Replace that first line (1) with

```rust
    registry.handle("edit.undo", |call, window, cx| {
        // While the Project view has the keyboard, Undo reverts the view's last delete or move first.
        if call.ctx.is_truthy("projectViewFocus") && workbench::undo_in_project_view(window, cx) {
            return Ok(());
        }
```

It applies the same whether or not 2.7 is in: the lines after it are untouched. With nothing for the view to revert, `undo_in_project_view` returns false and Undo goes on as before (the webview's `registerUndoHandler` returns `undoLastFileOperation()`'s false the same way). `use crate::workbench;` is already imported there. No existing test runs `edit.undo` with the tree focused (grep of `"edit.undo"`: `mod.rs`, `editor/command_actions.rs`, `editor/core_actions.rs`, `editor_language.rs`, all with an editor focused).

## 5. The delete seam (`crates/ide-host`)

### Why here, and what the webview does

- Webview: `deleteSelection` (`src/features/project-tree/operations.ts:449`) asks the Delete dialog (Safe delete checkbox, "Items are moved to the Trash."), then `projectFs(root).delete(p)` → IPC `fs_delete` (`src-tauri/src/commands/fs.rs:83`, `fs::delete(&state.host, window.label(), path)`) → `ide_host::ops::fs::delete` (`crates/ide-host/src/ops/fs.rs:137`): Local History `record_deletion` of every `deletion_paths` entry, then `ide_workspace::file_ops::delete` → `ide_vfs::delete` = `ops::delete(path, !cfg!(test))` → `trash::TrashContext::delete` (macOS `NsFileManager`), falling back to a permanent delete when the Trash refuses. Undo: `undoLastFileOperation` → `undoDelete` (`operations.ts:521`): `history_list_dir` + `history_restore` per path, the parent re-read, the first restored path revealed; "Some items could not be restored" otherwise.
- Native: `project_files.rs` answers `FsRequest::Delete` with `ide_host::ops::fs::delete(&host, &label, path)` (`BackendCall::FsDelete`), so the seam belongs in `ops::fs::delete`, which every Trash delete goes through (measured: `grep -rn 'file_ops::delete\|ide_vfs::delete' crates/ide-host/src crates/ide-workspace/src` finds only `ops/fs.rs:146`, `file_ops.rs:15` and the Unity `assets.rs:173`, both below it; callers of `ops::fs::delete`: `project_files.rs`, `conflicts.rs:164`, `log_actions.rs:1085`, the RPC server's `fs_delete`). Deletes inside a workspace edit (`FileEdit::Delete` of a refactoring or a language server, `ide-workspace/src/documents/workspace_edit.rs`) never use the Trash and are not affected. `cfg!(test)` in `ide_vfs` holds only for ide-vfs's own unit tests.
- Chosen: a per-`Host` override, set with `Host::set_trash`, following `set_mcp_executable` (no `HostConfig` field, so `src-tauri` and `openidea-server` do not change). Unset (the default, and always in the Tauri app), the delete is exactly today's. Set, it receives every path `deletion_paths` lists (a Unity asset and its `.meta`), after Local History has kept them, and nothing falls through to the system Trash or to a permanent delete.

### E1. The type (new)

In `crates/ide-host/src/lib.rs`, after (1)

```rust
pub type SinkFactory = Box<dyn Fn(&str) -> SharedSink + Send + Sync>;
```

add

```rust

/// What a delete does with a path in place of moving it to the system Trash ([`Host::set_trash`]).
pub type TrashFn = Arc<dyn Fn(&std::path::Path) -> IdeResult<()> + Send + Sync>;
```

### E2. The field (new)

After (1) `    pub(crate) mcp_executable: RwLock<Option<PathBuf>>,` add:

```rust
    /// Where deletes go instead of the system Trash; `None` (the default): the system Trash.
    trash: RwLock<Option<TrashFn>>,
```

### E3. Its initialiser (new)

After (1) `            mcp_executable: RwLock::default(),` add:

```rust
            trash: RwLock::default(),
```

### E4. Setter and getter (new)

After (1)

```rust
    pub fn set_mcp_executable(&self, executable: Option<PathBuf>) {
        *self.mcp_executable.write() = executable;
    }
```

add

```rust

    /// Puts `trash` in the place of the system Trash for every delete of this host from now on
    /// (`None`: the system Trash again). It is given each path a delete removes, once Local History
    /// has kept its contents. Tests use it so that no delete reaches the user's Trash.
    pub fn set_trash(&self, trash: Option<TrashFn>) {
        *self.trash.write() = trash;
    }

    /// Where deletes go instead of the system Trash, if anywhere.
    pub(crate) fn trash_override(&self) -> Option<TrashFn> {
        self.trash.read().clone()
    }
```

### F1. `ops::fs::delete` (new)

In `crates/ide-host/src/ops/fs.rs`, replace (1)

```rust
/// Moves to the trash; Local History keeps the contents so deleted files can be restored.
pub async fn delete(host: &Host, window: &str, path: PathBuf) -> IdeResult<()> {
    let history = host.project(window).ok().map(|p| p.history.clone());
    host.blocking(move || {
        let paths = ide_workspace::file_ops::deletion_paths(&path)?;
        if let Some(h) = history {
            for p in paths {
                h.record_deletion(&p, DELETE_SNAPSHOT_LIMIT);
            }
        }
        ide_workspace::file_ops::delete(&path)
    })
    .await
}
```

with

```rust
/// Moves to the trash (or where [`Host::set_trash`] says); Local History keeps the contents so
/// deleted files can be restored.
pub async fn delete(host: &Host, window: &str, path: PathBuf) -> IdeResult<()> {
    let history = host.project(window).ok().map(|p| p.history.clone());
    let trash = host.trash_override();
    host.blocking(move || {
        let paths = ide_workspace::file_ops::deletion_paths(&path)?;
        if let Some(h) = history {
            for p in &paths {
                h.record_deletion(p, DELETE_SNAPSHOT_LIMIT);
            }
        }
        match trash {
            // Every path the project's rules delete together (an asset and its `.meta`).
            Some(trash) => paths.iter().try_for_each(|p| trash(p.as_path())),
            None => ide_workspace::file_ops::delete(&path),
        }
    })
    .await
}
```

### F2. Its unit test (new)

At the end of `crates/ide-host/src/ops/fs.rs`, after (1)

```rust
        None => Ok(ide_vfs::load_text(&path)?.text),
    })
    .await
}
```

add

```rust

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HostConfig;
    use ide_core::{IdeError, NullSink, error::ErrorKind};
    use std::path::Path;
    use std::sync::Arc;

    /// A delete goes where `set_trash` says instead of the system Trash: a file and a folder leave
    /// their place whole; a path that is not there is refused.
    #[tokio::test]
    async fn a_delete_goes_where_set_trash_says_instead_of_the_system_trash() {
        let root = tempfile::tempdir().unwrap();
        let host = Host::new(HostConfig {
            config_dir: root.path().join("config"),
            data_dir: root.path().join("data"),
            local_data_dir: root.path().join("data"),
            languages: Arc::new(ide_language::LanguageRegistry::load(None).unwrap()),
            syntax: Arc::default(),
            sink_for: Box::new(|_| Arc::new(NullSink)),
            owner: |window| window,
            runtime: tokio::runtime::Handle::current(),
            project_environment: false,
        });
        let trash = root.path().join("trash");
        std::fs::create_dir_all(&trash).unwrap();
        let into = trash.clone();
        host.set_trash(Some(Arc::new(move |path: &Path| {
            let name = path.file_name().ok_or_else(|| IdeError::invalid("a path without a name"))?;
            std::fs::rename(path, into.join(name)).map_err(IdeError::from)
        })));
        let dir = root.path().join("project/src");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        delete(&host, "w", dir.join("a.txt")).await.unwrap();
        assert!(!dir.join("a.txt").exists());
        assert_eq!(std::fs::read_to_string(trash.join("a.txt")).unwrap(), "a\n");
        delete(&host, "w", dir.clone()).await.unwrap();
        assert!(!dir.exists() && trash.join("src").is_dir());
        let error = delete(&host, "w", dir.join("gone.txt")).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::NotFound);
    }
}
```

(The setup is `ops/term.rs`' test's; `tempfile` and `tokio` with `macros` are dev-dependencies of ide-host.)

### The end-to-end test

`project_view_actions.rs` `a_confirmed_delete_moves_the_file_to_the_trash_and_undo_brings_it_back`: `host.set_trash` moves into `<temp home>/trash`; Delete… on `docs/notes.txt`, Enter in the Delete dialog (Safe delete on, as stored by default); waits until the file is gone from disk and from the tree's rows; reads it from the trash folder; no "Some items could not be deleted"; then, with the tree holding the keyboard, `edit.undo` (D): the file is back on disk with its text, listed and selected, no "Some items could not be restored". Undo runs the existing `undo_delete` flow, whose requests `project_files.rs` already answers (`Request::HistoryListDir` → `history_list_dir`, `Request::HistoryRestore` → `history_restore`, `FsRequest::CreateDir` for a folder).

## 6. Terminal

### How tests start shells

- Existing tests (`terminal/panel.rs`, `terminal/view.rs`, `workbench::tests::start`, and the `context_menus.rs`, `plugin_contributions.rs`, `app_actions.rs`, `proofs_editor_search_safe_delete.rs` fixtures): `crate::terminal::test_support::isolate_shells(home, cx)` sets Settings › Terminal › env (global, in the temp home) to `HOME`=`ZDOTDIR`=`<home>/shell-home` (empty `.zshrc`) and `HISTFILE=/dev/null`. The shell itself is still the account's login shell **with `-l`** (`ide_terminal::resolve_shell`: no args → `-l` for zsh/bash/sh/…), interactive in the PTY. That meets "temp HOME, no history file" but not "non-interactive, non-login"; the compiled tests are left as they are (read, not changed here).
- Drafted tests: `start` (so `isolate_shells` as above) **plus** `plain_shells`: writes `<home>/bin/plainsh` = `#!/bin/sh` + `while IFS= read -r line; do eval "$line"; done` (via `test_support::program`, mode 0755) and sets `terminal.shellPath` to it and `terminal.shellIntegration` to false (global). Read in `ide-terminal`: `resolve_shell` adds `-l` only for an argument-less shell named `zsh`/`bash`/`sh`/`fish`/`ksh`/`dash`/`tcsh`/`csh`, and `integration::apply` touches only `zsh`/`bash`/`fish`; `plainsh` is neither, so the PTY runs `/bin/sh <script>`: not a login shell, not interactive (a script operand), no startup file, no history (non-interactive shells keep none; `HISTFILE=/dev/null` besides). Run Command runs `/bin/sh -c …` / `/bin/sleep 30` (explicit arguments). New Predefined Session is given `plainsh`'s path (tab "plainsh"). Typing still works: the PTY's line discipline echoes, the script `eval`s each line (`exit` ends it).

### Split panes

Checked: §3. The draft's per-pane test (`the_splits_lay_their_shells_side_by_side_or_stacked_and_unsplit_joins_them`) runs on `plainsh`.

### `terminal.find`, the close-session question, view modes

- **`terminal.find`: stays out.** Webview: `FindBar.tsx` (173 lines: field, Match Case / Words / Regex toggles, counter, next/previous) on xterm's search addon, which searches the scrollback and decorates matches. Native: only the model (`terminal/find_bar.rs`: `TerminalFind`, `find_bar_key`, `match_counter`); `TerminalEvent::FindRequested` (⌘F in the terminal) is dropped in `TerminalPanel::terminal_event`, and `emulator.rs` has no scrollback search or match highlighting. Building it is a view plus a search over the emulator grid plus painting: not small.
- **Close-session question: built now** (`terminal_actions.rs`; A5). As `requestCloseSession` → `confirmEndSessions` (`src/features/terminal/store.ts:249–271`) with `isBusy` (`session.ts:624`): a session not running, or without a PTY, is not busy; one running a program of its own (`command && !command.shell`) is; otherwise the backend's `term_is_busy` (`ide_host::ops::term::is_busy`, foreground process group ≠ the shell), an error counting as not busy. Busy → `PromptDialog::confirm` with `terminate_confirmation` ("Process '{name}' Is Running" / "Do you want to terminate the process '{name}'?" / Terminate); confirmed → the tab closes (last one: the panel's `Emptied` hides the tool window), else the keyboard goes to the tab that shows. Test: `close_session_asks_before_terminating_a_running_command_and_closes_an_idle_shell_at_once` (Cancel keeps, Terminate closes, an idle `plainsh` closes without a question and the Terminal hides). Not built: ⌘W in the terminal still closes through `TerminalEvent::CloseRequested` → `TerminalPanel::terminal_event` without the question, and the tab strip's × (`requestCloseTab`) too: routing them needs a second `TerminalPanelEvent` variant, and `mount_panels` matches the event irrefutably (`let TerminalPanelEvent::Emptied = event;`), in the compiling agent's region.
- **View modes: stays out.** Nothing under `src/features/terminal/` has view modes (grep), so this reads as the Project view's `project.viewMode.*` (`actions.ts:177`, six panes: `VIEW_MODES`). Native: `ProjectViewState::set_view_mode` / `mode_shows` exist as models, but `ProjectTree` builds its rows from its own listings (`build_model`, `rows(model, expanded)`) and has no pane; `ExpandAll::start` already handles filtered panes for when it does. A pane needs the tree to draw a filtered model and a pane switcher: not small.

## 7. Menus and the literal row lists that change

Registering `navigate.selectIn`, `project.revealInFinder`, `project.openInTerminal`, `project.copyFileName`, `project.copyRelativePath` and `terminal.openAt` makes these rows show (`ui/popup_menu.rs` `resolve` hides rows of unregistered actions):

| Menu                          | Change                                                                                                                                       | Literal list asserted by                                                                              |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Editor (`context_menus.rs`)   | **Open In** › (Project View, Finder, Terminal) appears after Send to Agent, before Git                                                       | `ROOT` in `context_menus.rs` tests (G1–G3)                                                            |
| Editor tab (`tab_file_rows`)  | **Open In** › (same) after Add Bookmark, before Git; Copy Path/Reference… › gains File Name, Path From Content Root (`mod.rs` tab menu rows) | `a_tabs_menu_closes_pins_and_splits` after `context_menus_reconciled.md` 1h (H); submenu: none        |
| Project view (file or folder) | Open In › **Terminal** after Finder (`context_menu.rs:462`, `terminal.openAt` registered)                                                    | none (stage 3's plugin test checks only the row after Delete…; model tests set `ctx.actions` by hand) |
| Select In chooser (new)       | Select In, Project View, Finder, Terminal                                                                                                    | `select_in_lists_the_places_that_show_the_file_and_the_project_view_selects_it`                       |
| Menu bar (`menu_spec.rs`)     | Navigate › File Path, Select In… now run                                                                                                     | none                                                                                                  |

Cumulative `ROOT` once `editor_all` is in as well (derived by reading `the_editor_menu_lists_its_rows_in_order` and `editor_all_integration.md` §6: `code.fold`… Folding, `refactor.rename` Rename…, `code.generate` Generate…; Refactor › stays hidden, none of `refactor.thisMenu`/`refactor.move`… is registered): `["Show Context Actions", "Paste", "Copy / Paste Special", "Column Selection Mode", "Go To", "Folding", "Rename…", "Generate…", "Send to Agent", "Open In", "Git"]`, and `GO_TO` gains `Test` (`navigate.gotoTest`) where `context_menus.rs` puts it (`action("navigate.gotoTest", "Test")`). **Flag:** `editor_all_integration.md` §10 item 10 says no test asserts a fixed list of those rows; the reconciled `context_menus.rs` tests do (`ROOT`, `GO_TO`), so whoever applies `editor_all` updates them; G1–G3 are written to apply on either version.

### G1. `workbench/context_menus.rs`: the comment over `ROOT` (new)

Replace (1)

```rust
    /// registers: Folding, Rename…, Refactor, Generate…, Open In, Local History and the Compare rows
```

with

```rust
    /// registers: Folding, Rename…, Refactor, Generate…, Local History and the Compare rows
```

(If the `editor_all` integrator rewrote this comment, drop "Open In" from whatever list it has.)

### G2. `ROOT`'s length (new)

On the line `    const ROOT: [&str; N] = [` (1; `N` is 7 today, 10 if `editor_all`'s rows were added), add one to `N`.

### G3. `ROOT`'s rows (new)

Replace (1)

```rust
        "Send to Agent",
        "Git",
    ];
```

with

```rust
        "Send to Agent",
        "Open In",
        "Git",
    ];
```

### H. `workbench/mod.rs`: `a_tabs_menu_closes_pins_and_splits`, on top of `context_menus_reconciled.md` 1h (new)

1h's replacement text (quoted from that note, 1 there; 0 in `mod.rs` until 1h is applied, then 1):

```rust
                "Split Down",
                // The file's rows whose actions are registered: the bookmark group, Git, Copy Path/Reference…
                // (Copy Absolute Path) and Rename File…
                "Add Bookmark",
                "Git",
```

Replace it with

```rust
                "Split Down",
                // The file's rows whose actions are registered: the bookmark group, Open In, Git, Copy
                // Path/Reference… (Copy Absolute Path, File Name, Path From Content Root) and Rename File…
                "Add Bookmark",
                "Open In",
                "Git",
```

The rest of that test holds: its disabled rows are still only Close Tabs to the Left (a submenu row is never greyed), `"down down down enter"` reaches the rows above Split Down, and no new row starts with P (`"p enter"` still Pin Tab).

## 8. Services and Docker

- No drafted action starts `docker`: `services_actions.rs` registers nothing; `terminal_actions.rs` / `project_view_actions.rs` never reach `ide_docker`.
- The tests: showing Services calls `DockerStore::watch` → `load_settings` → for each connection `DockerRequest::Snapshot` → `ide_docker::snapshot` (which resolves the process-wide `docker_executable()` and runs `docker version` and listings if one is found). The default `DockerSettings` (`ide-docker/src/connection.rs:148`, used when `docker.json` is missing) holds one connection (`DockerEndpoint::Default`). `services_project` writes `<data>/docker.json` = `{ "connections": [], "registries": [] }` (an explicit empty list: the serde default applies only to a missing key) before the window opens, so `connections()` is empty (read: `settings_loaded` stores the loaded `DockerSettings` as they are and then calls `refresh_all`, which maps over `connections()`), `refresh_all` / `poll` / `refresh` ask for nothing, and no Docker program is looked up or run, installed or not. The test asserts no `docker:` row (`CONNECTION_PREFIX`) is listed.
- The Docker view toggles' test never shows Services and also uses the empty `docker.json`.
- Compiled tests with the same exposure, left unchanged (read, not changed): `workbench/tests/smoke_services.rs`, `workbench/tests/proofs_services.rs` and `mod.rs`' `a_run_configuration_added_to_services_runs_from_its_row_into_the_detail_pane_and_stop_ends_a_service` show Services with the default connection, so with a `docker` on the machine they run its read-only `docker version` / listings. Writing the same `docker.json` in `tests::run_project` closes that for all of them (one hunk in `mod tests`, not part of this file).
- Every Docker operation stays unregistered (`docker.start`, `stop`, `restart`, `remove`, `showDashboard`, `showLog`, `openLogInEditor`, `showProcesses`, `showLayers`, `inspect`, `execShell`, `pullImage`, `prune`, the toolbar ids, `docker.refresh`'s operations): a test cannot confine `docker_executable()` (a `static OnceLock` from `OPENIDEA_DOCKER`, `which docker`, fixed paths) to a fake, and `carry_out_docker`'s `other =>` arm drops `ContainerOp`, `Remove`, `Stream`, `Inspect`, `ConfirmDelete`, `PromptPull`, `ConfirmPrune`. Building them needs an executable on `DockerEndpoint` or on `Host`, read per call, first.

## 9. Reveal in Finder, Open in Terminal, Open In

- **Reveal in Finder** (`project.revealInFinder`, `actions.ts:391`; Open In › Finder rows): the webview calls `revealInFileManager` (`src/platform/fileManager.ts:47`) → the Tauri opener plugin's `revealItemInDir` (no `src-tauri/src/commands/` adapter; the plugin's own IPC), "Cannot reveal in Finder" on failure. Native: `reveal_in_file_manager` → `cx.reveal_path` (gpui's platform call; no error comes back, so no failure notice: partial). Tested without Finder: `FileManager` global seam; `record_reveals` puts a recorder in it (the gpui test platform's `reveal_path` is `unimplemented!()`, so any unseamed reveal in a test would panic). Every Reveal row (File Path, Select In, Open In › Finder in the tree and editor menus) goes through B9.
- **Open in Terminal** (`project.openInTerminal`, `actions.ts:400`): stays in the app; delegates to `terminal.openAt` (IDE terminal tab in the folder). Tested end to end on `plainsh`.
- **Open In › Browser** (the `openIn` menu group of `src/features/web/index.ts:120`, `web.openInBrowser`): leaves the app through the browser. Not registered: there is no web feature natively. When it is built, gpui's test platform records `open_url` (`opened_url`), so a test can assert the URL without launching a browser.

## 10. Removed because it exists, and what stays unregistered

Removed / not duplicated:

- The draft's §1c `pub(super)` hunks: the helpers it needed are reachable (`start`, `wait_long`, `run_action`, `click_on`, `notification_titles` are `pub(super)`; `plugins_window::test_support::trusted_project`/`program` exist since plugin stages 1–2).
- `project.copy`/`cut`/`paste`, `project.delete`, `project.copyAbsolutePath`, `project.copyElement`, `project.undoMove`, `editor.copyReference`, `editor.copyPathReference`: registered by `project_files.rs`; the drafts only run them.
- `docker.toggleStoppedContainers`/`toggleUntaggedImages`: registered by `services_window.rs`; the test only runs them.
- `vcs.addToVcs`, the ignore actions, Rollback… on Project view selections (Git slice `88677f14`), `focusedDirectory` (`e22ffae7`): untouched; nothing here registers them. `run.services.runAll`/`restartAll`/`stopAll`: covered by `smoke_services.rs`; not duplicated.
- `terminal.closeSession`: registered once, in `terminal_actions.rs` (A5 deletes the `mod.rs` one).

Not registered, with why: `project.refresh` / `project.reloadFromDisk` (the file watcher re-reads the same folders, so a test cannot tell the action's read from the watcher's; `ProjectTree::reload_all` exists), `project.renameInline` (no inline editor), `project.findUsages` (needs a language server; `proofs_editor_search_safe_delete.rs` runs the context menu's Find Usages row, `ProjectCommand::FindUsages`, with a fake one), `project.showHistory` (`vcs.localHistory` unregistered), `project.reformatCode` / `optimizeImports` (rows appear once `editor_all` registers `code.reformatFile` / `optimizeImportsFile`; the hidden actions themselves are not registered here), `project.viewMode.*` (§6), the other six `project.option.*` (the tree does not act on them), `terminal.find` (§6), `terminal.pluginSession` (no plugin terminal request reaches this window), every `docker.*` operation (§8).

## 11. Ids, conditions, labels, keys

Every id, `when`, `keyWhen` and title comes from `assets/gen/actions.json` at run time through `registry.handle` (measured: the entries below), except the computed ones (`project.revealInFinder`, `project.option.*`, `project.sortBy.*`), whose metas `project_view::actions::project_actions` builds exactly as `actions.ts:156–198` (`when: projectOpen` for Reveal, none for the toggles, as the webview). Keys are `keymaps.json`'s (intellij-macos / intellij-windows, the default presets the tests run on), measured. Scanned over all six presets: no other preset binds an id that is unbound here; vscode binds only `project.open` (⌘↓ on mac), eclipse binds `navigate.selectIn` (⌥⌘W mac / ⌥F1 win), `project.open` (F3 / F12, ⇧Ctrl+E), `project.expandAll` / `collapseAll` (⌘⇧ / Ctrl+⇧ + numpad * and /), `project.compareWith` (⌘D / Ctrl+D) and `navigate.showFilePath` (Ctrl+Alt+F12, win); the keymap's `keyWhen` / `when` confine them the same way, untested on those presets:

| Id                                                                                                                                                                                                                                                                         | when / keyWhen                                                                       | Key (mac / win)                                                                                                                                                      | Shares the key with (dispatcher: last binding first, first enabled wins)          | Test: runs where true / does not where false                                                           |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `project.expandAll`                                                                                                                                                                                                                                                        | `projectOpen` / `projectViewFocus`                                                   | ⌘= / Ctrl+=                                                                                                                                                          | `code.unfold` (editor), `image.zoomIn`                                            | `expand_all_and_collapse_all_…_act_in_the_tree_only`: runs in the tree; not in the editor              |
| `project.collapseAll`                                                                                                                                                                                                                                                      | `projectOpen` / `projectViewFocus`                                                   | ⌘- / Ctrl+-                                                                                                                                                          | `code.fold`, `image.zoomOut`                                                      | same test                                                                                              |
| `project.open`                                                                                                                                                                                                                                                             | `projectViewFocus`                                                                   | F4, ⌘↓ / F4                                                                                                                                                          | `debug.jumpToSource`, `diff.jumpToSource`, `db.editData`, `find.popup.openResult` | `jump_to_source_opens_the_selected_file`: F4 in the tree runs it; in the editor not                    |
| `project.compareWith`                                                                                                                                                                                                                                                      | `projectViewFocus`                                                                   | ⌘D / Ctrl+D                                                                                                                                                          | `code.duplicateLine`, `vcs.showDiff` (`!editorTextFocus`), `db.duplicateSource`   | `compare_with_…`: ⌘D in the tree runs it (path prompt); in the editor neither the action nor its key   |
| `navigate.selectIn`                                                                                                                                                                                                                                                        | `projectOpen`                                                                        | ⌥F1                                                                                                                                                                  | none                                                                              | `select_in_lists_…` (runs in the editor); `the_navigation_choosers_keys_run_nothing_without_a_project` |
| `navigate.showFilePath`                                                                                                                                                                                                                                                    | `projectOpen`                                                                        | ⌥⌘F12 / Ctrl+Alt+F12                                                                                                                                                 | none                                                                              | `file_path_lists_…` (runs); `the_navigation_choosers_keys_run_nothing_without_a_project`               |
| `navigate.selectInProjectView`, `project.reveal`, `project.copyPath`, `project.copyFileName`, `project.copyRelativePath`, `project.copySourceRootPath`, `project.openInTerminal`, `project.findInFolder`, `project.revealInFinder`, `project.option.*`, `project.sortBy.*` | `projectOpen` (copy forms: `projectOpen \|\| textEditorActive` / `projectViewFocus`) | none bound                                                                                                                                                           | —                                                                                 | —                                                                                                      |
| `terminal.openAt`, `newPredefinedSession`, `runCommand`, `splitRight`, `splitDown`, `splitAndMoveRight`, `splitAndMoveDown`, `unsplit`                                                                                                                                     | `projectOpen`                                                                        | none bound                                                                                                                                                           | —                                                                                 | `the_terminal_actions_need_a_project` (refused without a project)                                      |
| `terminal.closeSession`                                                                                                                                                                                                                                                    | `terminalFocus`                                                                      | none in `keymaps.json`; ⌘W in a focused terminal is the terminal's own key (`terminal/keys.rs` `CLOSE_SESSION_ACTION`), which today closes without the question (§6) | —                                                                                 | `close_session_…` runs it with the terminal focused                                                    |

Menu places: Navigate › File Path and Select In… (`menu_spec.rs`, existing); the editor, tab and tree menus' Open In (§7); the Project view's header (Expand All, Collapse All, options gear) and the terminal tab's context menu (Split Right / Down, `TerminalView.tsx:122`) are not built natively, so those actions have no menu place yet.

## 12. Anchors in regions the compiling agent is likely to touch

Re-count with `anchors.py` right before pasting. `workbench/mod.rs`: A1–A3 (the `mod` block, where every stage adds lines), A4 and A5 (`register_actions`, which stages 1–3 and `app_actions` extend), H (inside `mod tests`, on `context_menus_reconciled.md` 1h's text). `workbench/project_files.rs`: B1–B9 (plugin stage 3.6 rewrote the region around `with_menu_context` and `tree_context_menu`; `context_menus_reconciled.md` 3.1/3.2 edit it next). `workbench/context_menus.rs` (G1–G3, the reconciled draft the integrator declares, whose `ROOT` the editor stage may also change). Outside the compiling agent's stated area: `terminal/panel.rs` (C), `editor/actions.rs` (D, after `editor_all` 2.7), `crates/ide-host` (E, F).

## 13. Assumptions to check, in order

1. The pending notes are applied as written: plugin stage 3.6 (`with_contributed_menu_context`, keeping `fn with_menu_context<R>(…` as one line), `context_menus_reconciled.md` 1h (H's anchor) and its `context_menus.rs` (G's anchors), `editor_all_integration.md` 2.7 (D's first line unchanged). Re-run `anchors.py` right before pasting.
2. gpui's `Keystroke::parse` reads `cmd-=`, `cmd--`, `alt-f1`, `cmd-alt-f12`, `f4`, `cmd-d` and `platform/input.rs` turns them into the keymap's strokes. On Linux CI the keymap is intellij-windows; `primary()` and the File Path key switch to `ctrl-` there.
3. A key pressed in `Workbench::new` without a project and without a focused element still reaches the dispatcher (window-level key listener).
4. `layout.show(TERMINAL_TOOL_WINDOW, true)` mounts the panel synchronously (read: `with_layout` → `mount_panels`, `cx.new(TerminalPanel::new)`), so `terminal_panel()` finds it right after.
5. `parse_shell_command` keeps a path without spaces as one word (`quote_path` quotes otherwise); the temp home has no space on macOS.
6. `ide_host::ops::project::roots(..).roots` includes the user's marks right after `settings::set`.
7. A disabled `MenuItem::entry(title, None)` is not selectable, so "Project View" / the file is the first selected row (the Select In and File Path literal lists).
8. Toggle state is read from `UiStorage` (`ViewOptions::stored`), which `ProjectTree::set_options` writes.
9. `cx.did_prompt_for_paths()` is false again once `simulate_path_prompt_response` answered.

## 14. Compile risks

- `start_flow`'s `Option<Done>` from the sibling: B1 makes the alias `pub(super)` with the function (aliases are transparent, so this is belt and braces).
- `on_backend`'s `+ use<T, F, W>` (copied from `project_files.rs`' `host_call`, which compiles).
- Calling an `Arc<dyn Fn(&Path) -> …>` / `Rc<dyn Fn(&Path, &mut App)>` with call syntax (`trash(p.as_path())`, `reveal(Path::new(&path), cx)`): relies on auto-deref of the callee; if it trips, write `(*trash)(…)`.
- `Some(Arc::new(move |path: &Path| …))` passed to `set_trash(Option<TrashFn>)`: the unsized coercion happens at `Some`'s argument; if the closure's return type is not inferred, annotate `-> IdeResult<()>`.
- `close_terminal_session`: `panel.read(cx)` is shadowed into `panel` and its borrows end before `cx.spawn_in` (NLL); if the borrow checker objects, compute the values in a block.
- `end_terminal_tab`'s early `return self.close_terminal_tab(…)` returns `()`; clippy does not flag it (`needless_return` is about the last statement).
- `test_support::program` is `#[cfg(unix)]`: the terminal tests use `/bin/sh` anyway and do not build on Windows.
- `primary("d")` etc. return `String`; `actions_run_by(&primary(…), cx)`.

## 15. Tests most likely to fail first, and why

1. **Key tests** (`expand_all_…`, `compare_with_…`, `jump_to_source_…`, `select_in_…`, `file_path_…`): `actions_run_by` assumes the keyboard path calls `ActionRegistry` listeners (`run` calls them at nesting 0 for every source) and that `simulate_keystrokes("cmd-=")` / `"cmd--"` / `"alt-f1"` / `"cmd-alt-f12"` reach the dispatcher as `Meta+=`, `Meta+-`, `Alt+F1`, `Meta+Alt+F12` (assumption 2). The negative half of `the_navigation_choosers_keys_run_nothing_without_a_project` cannot fail for the wrong reason, but it also proves little if no element has the keyboard (assumption 3).
2. **`a_confirmed_delete_moves_the_file_to_the_trash_and_undo_brings_it_back`**: the safe delete's usage search must finish in Light mode (the compiled `a_confirmed_delete_searches_deletes_…` shows it does); Local History must hold the snapshot (`record_deletion` runs before the seam); `edit.undo` must see `projectViewFocus` after `select_in_tree` of `docs`.
3. **`the_splits_lay_their_shells_…`**: pane bounds need a paint after each split (`pane_bounds` runs `run_until_parked`; if `debug_bounds` is empty, `cx.update(|window, _| window.refresh())` first).
4. **`close_session_asks_…`**: `terminalFocus` must be published when `run_action` reads the context (after `open_terminal_tab`'s focus, after Cancel the test refocuses the terminal); the idle `plainsh` must not be "busy" (`foreground_busy`: the PTY's foreground group is the script's own, since a non-interactive sh does no job control).
5. **`the_copy_path_forms_…`** and **`an_excluded_folder_…`**: walking the context menu with arrows (`choose_in_menu`), `project-row:{path}` selectors (row key = path for plain files), `gen` not excluded by detection.
6. **`move_up_and_move_down_…`**: the stored order's format (ids quoted inside the stored JSON).

## 16. Action → end-to-end test → IPC (adapter, backend function) → status

| Action id                                                                                              | End-to-end test                                                                                                                                     | IPC it stands for (`src-tauri/src/commands/` → backend)                                                                                                                  | Status                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `navigate.selectInProjectView`                                                                         | `select_in_project_view_selects_the_edited_file_and_reveal_leaves_the_keyboard_where_it_was`                                                        | `fs_read_dir` (`fs.rs` → `ide_host::ops::fs::read_dir`) for the folders on the way                                                                                       | done                                                                                                                                                                      |
| `project.reveal`                                                                                       | same                                                                                                                                                | `fs_read_dir`                                                                                                                                                            | done                                                                                                                                                                      |
| `navigate.selectIn`                                                                                    | `select_in_lists_the_places_that_show_the_file_and_the_project_view_selects_it`, `the_navigation_choosers_keys_run_nothing_without_a_project`       | `fs_read_dir`                                                                                                                                                            | partial: chooser at the top of the window, not at the caret; no contributed targets (Structure)                                                                           |
| `navigate.showFilePath`                                                                                | `file_path_lists_the_file_then_every_folder_up_to_the_root`, `the_navigation_choosers_keys_run_nothing_without_a_project`                           | none; a row reveals (opener plugin `revealItemInDir` → `cx.reveal_path`)                                                                                                 | partial: rows show names only, not the folder path beside them (`MenuItem` has no detail column)                                                                          |
| `project.revealInFinder`                                                                               | `reveal_in_finder_shows_the_selection_or_the_edited_file_in_the_file_manager`                                                                       | opener plugin `revealItemInDir` (no `commands/` adapter) → `cx.reveal_path`                                                                                              | partial: no "Cannot reveal in Finder" notice (gpui reports no failure); WSL path translation not needed natively                                                          |
| `project.expandAll`                                                                                    | `expand_all_and_collapse_all_open_and_close_every_folder_and_their_keys_act_in_the_tree_only`                                                       | `fs_read_dir` per folder, at most 400                                                                                                                                    | done (the tree's numpad `*` key not handled)                                                                                                                              |
| `project.collapseAll`                                                                                  | same                                                                                                                                                | none                                                                                                                                                                     | done                                                                                                                                                                      |
| `project.copyPath`, `project.copyFileName`, `project.copyRelativePath`, `project.copySourceRootPath`   | `the_copy_path_forms_copy_the_selection_and_the_source_root_path_follows_mark_directory_as`                                                         | `project_roots` (`app.rs` → `ide_host::ops::project::roots`), then the clipboard                                                                                         | done                                                                                                                                                                      |
| Mark Directory as (context menu row, `Effect::MarkDirectory`)                                          | same; `an_excluded_folder_shows_as_excluded_and_the_search_skips_it`                                                                                | `project_roots`, `settings_set` (`app.rs` → `ops::project::set_settings`, scope project), `fs_read_dir`; the search: `search_start` (`search.rs` → `ops::search::start`) | partial: no "Unmark as …" / "Cancel Exclusion" for a marked folder (`with_menu_context` passes `roots: &[]`)                                                              |
| `project.compareWith`                                                                                  | `compare_with_shows_two_files_side_by_side_and_asks_for_the_second_of_one`                                                                          | `fs_read_text` (`fs.rs` → `ops::fs::read_text`, through `open_diff`)                                                                                                     | partial: the webview delegates to `diff.compareFiles` / `diff.compareWith` (unregistered natively); here two files open the diff directly and one asks with the OS picker |
| `project.open`                                                                                         | `jump_to_source_opens_the_selected_file`                                                                                                            | `doc_open` (`doc.rs` → `ops::doc::open`, through `open_project_file`)                                                                                                    | done                                                                                                                                                                      |
| `project.openInTerminal`                                                                               | `open_in_terminal_on_a_folder_opens_a_shell_there_named_after_it`                                                                                   | `term_spawn` (`term.rs` → `ops::term::spawn`, from `TerminalView::new`) via `terminal.openAt`                                                                            | done                                                                                                                                                                      |
| `project.findInFolder`                                                                                 | `find_in_folder_opens_find_in_files_on_the_selected_folder`                                                                                         | delegates to `find.inPath` (`search_start` when run)                                                                                                                     | done                                                                                                                                                                      |
| `project.option.{showHidden,showExcluded,compact,foldersFirst}`, `project.sortBy.{name,type,modified}` | `the_view_options_change_what_the_tree_shows_and_say_whether_they_are_on`                                                                           | none (UI storage)                                                                                                                                                        | partial: the six other options and `project.viewMode.*` unregistered (§6, §10)                                                                                            |
| `project.delete` (existing) + seam                                                                     | `a_confirmed_delete_moves_the_file_to_the_trash_and_undo_brings_it_back`; ide-host `a_delete_goes_where_set_trash_says_instead_of_the_system_trash` | `fs_delete` (`fs.rs` → `ops::fs::delete` → `file_ops::delete` / `Host::set_trash`)                                                                                       | done for a file (folders: unit test only)                                                                                                                                 |
| `edit.undo` in the Project view (Undo of delete / move)                                                | same                                                                                                                                                | `history_list_dir`, `history_restore` (`history.rs` → `ops::history::*`), `fs_create_dir` for folders; a move: `project.undoMove` → `refactor_undo_file_move`            | partial: ⌘Z pressed in the tree is not handled (the tree's `key_down` ignores modified keys, `edit.undo` has no binding); Edit › Undo / the action is                     |
| `project.cut` / `project.paste` / `project.copy` (existing)                                            | `cut_and_paste_moves_the_file_and_a_copy_onto_a_taken_name_asks_for_another`                                                                        | `fs_rename`/refactor move, `fs_copy`                                                                                                                                     | partial: files are not handed to the OS clipboard (`OsClipboardWrite::Files` dropped)                                                                                     |
| `terminal.openAt`                                                                                      | `open_in_terminal_on_a_folder_opens_a_shell_there_named_after_it`                                                                                   | `term_spawn`                                                                                                                                                             | done                                                                                                                                                                      |
| `terminal.newPredefinedSession`                                                                        | `run_command_runs_its_program_in_a_tab_of_its_own_and_a_predefined_session_runs_the_shell`                                                          | `term_spawn`                                                                                                                                                             | done                                                                                                                                                                      |
| `terminal.runCommand`                                                                                  | same                                                                                                                                                | `term_spawn`                                                                                                                                                             | done (makes `github.rs`' "not registered" fallback dead code; "Log in with gh" now runs `gh auth login` in a tab: no test clicks it)                                      |
| `terminal.splitRight`, `splitDown`, `splitAndMoveRight`, `splitAndMoveDown`, `unsplit`                 | `the_splits_lay_their_shells_side_by_side_or_stacked_and_unsplit_joins_them`, `the_terminal_actions_need_a_project`                                 | `term_spawn` for a new pane's shell; `unsplit`: none                                                                                                                     | partial: one tab strip for every group; split handles not draggable; no tab context menu (Split rows)                                                                     |
| `terminal.closeSession`                                                                                | `close_session_asks_before_terminating_a_running_command_and_closes_an_idle_shell_at_once`                                                          | `term_is_busy` (`term.rs` → `ops::term::is_busy`); the end: `term_kill` when the view drops                                                                              | partial: ⌘W in the terminal and the tab's × close without the question                                                                                                    |
| Services Move Up / Move Down (toolbar items, no action id)                                             | `move_up_and_move_down_reorder_a_configuration_within_its_type_and_store_the_order`                                                                 | none (UI storage `order_key`)                                                                                                                                            | partial: the next Services window's order is not reopened in the test                                                                                                     |
| `docker.toggleStoppedContainers`, `docker.toggleUntaggedImages` (existing)                             | `the_docker_view_options_flip_and_store_what_the_tree_leaves_out`                                                                                   | none (UI storage `FILTERS_KEY`)                                                                                                                                          | done                                                                                                                                                                      |
| `terminal.find`, `docker.*` operations, `project.viewMode.*`                                           | none                                                                                                                                                | —                                                                                                                                                                        | missing (§6, §8)                                                                                                                                                          |
