# Integrating the Project view's, the Terminal's and the Services' remaining actions

New files, not compiled yet:

- `app-gpui/src/workbench/project_view_actions.rs` (Project view: Select In, Select in Project View, Reveal, File Path, Expand/Collapse All, the Copy Path forms, Compare With, Jump to Source, Open in Terminal, Find in Folder, the view-option toggles, Mark Directory as).
- `app-gpui/src/workbench/terminal_actions.rs` (Open in Terminal, New Predefined Session, Run Command in Terminal, Split Right/Down, Split and Move Right/Down, Unsplit).
- `app-gpui/src/workbench/services_actions.rs` (tests only: Move Up / Move Down end to end).

No new view file. All three are child modules of `workbench`.

Per module:

- **project_view_actions**:
  - Registers 21 actions: `navigate.selectInProjectView`, `project.reveal`, `navigate.selectIn`, `navigate.showFilePath`, `project.expandAll`, `project.collapseAll`, `project.copyPath`, `project.copyFileName`, `project.copyRelativePath`, `project.copySourceRootPath`, `project.compareWith`, `project.open`, `project.openInTerminal`, `project.findInFolder`, `project.option.{showHidden,showExcluded,compact,foldersFirst}` and `project.sortBy.{name,type,modified}`.
  - Adds `Workbench::mark_directory_as`, which the context menu's Mark Directory as reaches through hunk 2b.
  - 11 end-to-end tests.
  - Needs sections 1, 1c and 2.
- **terminal_actions**:
  - Registers 8 actions: `terminal.openAt`, `terminal.newPredefinedSession`, `terminal.runCommand`, `terminal.splitRight`, `terminal.splitDown`, `terminal.splitAndMoveRight`, `terminal.splitAndMoveDown` and `terminal.unsplit`.
  - 4 end-to-end tests and 1 unit test.
  - Needs sections 1, 1c and 3.
- **services_actions**:
  - Registers nothing.
  - 2 end-to-end tests: Move Up / Move Down through the toolbar, and the two Docker view toggles through the registry.
  - Needs sections 1a and 1c. Every anchor below is quoted from the file as it is now; no line numbers (`mod.rs` moves). Run `cargo fmt` after pasting: several pasted lines run past 120 columns.

## 1. `app-gpui/src/workbench/mod.rs`

### 1a. The modules

After `mod project_files;` add:

```rust
mod project_view_actions;
```

After `mod services_window;` add:

```rust
mod services_actions;
```

Before `mod todo_window;` add:

```rust
mod terminal_actions;
```

### 1b. The actions

In `pub fn register_actions(cx: &mut App)`, after `project_files::register_actions(registry);` add:

```rust
    project_view_actions::register_actions(registry);
    terminal_actions::register_actions(registry);
```

None of the ids is registered anywhere else today (grep of every `"<id>"` in `app-gpui/src` outside `project_view/` and the new files: only callers, menus and keymaps). `register` replaces silently, so check again after other drafts land: `diff.compareWith` and `navigate.navigationBar` are the ones another draft may add (both are only read here, never registered).

### 1c. Test helpers the new test modules use

The new tests import from `super::super::tests`, as `vcs_operations.rs` does. Make these `fn`s of `mod tests` in `mod.rs` `pub(super)` (each anchor is the whole first line):

- `    fn start(cx: &mut TestAppContext) -> tempfile::TempDir {`
- `    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {`
- `    fn trusted_project<'a>(`
- `    fn program(id: &str, command: &str, arg: &str) -> ide_run::RunConfig {`
- `    fn run_project(`
- `    fn project_tree_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ProjectTree> {`
- `    fn text_of(path: &std::path::Path) -> String {`
- `    fn select_in_tree(workbench: &Entity<Workbench>, path: &std::path::Path, cx: &mut VisualTestContext) {`
- `    fn tree_labels(tree: &Entity<ProjectTree>, cx: &mut VisualTestContext) -> Vec<String> {`

`run_action`, `wait_long`, `click_on` and `notification_titles` are `pub(super)` already.

## 2. `app-gpui/src/workbench/project_files.rs`

### 2a. Visibility for the sibling module

In `impl Workbench`:

- `    fn with_view<R>(` → `    pub(super) fn with_view<R>(`
- `    fn start_flow(` → `    pub(super) fn start_flow(`
- `    fn nodes_for(&mut self, arg: Option<&Value>, cx: &mut Context<Self>) -> Vec<Arc<ProjectNode>> {` → `pub(super) fn nodes_for(…`
- `    fn active_file_path(&self, cx: &App) -> Option<String> {` → `pub(super) fn active_file_path(…`
- `    fn with_menu_context<R>(&self, cx: &App, build: impl FnOnce(&MenuContext<'_>, &App) -> R) -> R {` → `pub(super) fn with_menu_context<R>(…`
- `    fn run_project_command(` → `    pub(super) fn run_project_command(`

At the top, `type Done = Box<…>;` becomes `pub(super) type Done = Box<dyn FnOnce(&mut Workbench, Outcome, &mut Window, &mut Context<Workbench>)>;`. The new module passes `None` for it; the alias appears in `start_flow`'s signature, so it gets the same visibility as the function.

### 2b. Mark Directory as is carried out

In `fn carry_out_effect`, replace

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

If `NotificationRequest` / `NotificationLevel` become unused in `project_files.rs` after this (they are still used by `run_flow_action`, so they should not), drop them from its `use`.

## 3. `app-gpui/src/terminal/panel.rs`

None of the eleven Terminal actions fits the panel's public API: `terminals` is private, `start_active` starts only the active session, and `render` draws only `active_view()`. These hunks add a mutator that ends in `carry_out`, start the shell of every group's selected tab, and draw the group layout.

### 3a. Imports

Replace

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

and in the `gpui_kit` import add `AnyElement` (the list becomes `AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString, Subscription, Window, div, prelude::*, px,`).

### 3b. The accessors and the mutator

After

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

### 3c. Every pane's shell starts

Replace the whole `fn start_active` (from `    /// Starts the shell of the active tab's session if it has none yet. A tab that is not` to its closing `    }` after `self.terminals.set_session_state(&key, state);`) with

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

With one group this starts exactly what the old code started: the group's selected tab is the active tab. `remembered_tabs_start_their_shells_when_first_shown` keeps passing for that reason (one group: 1 started, then 2 once the first tab is selected).

### 3d. The groups are drawn

In `fn render`, replace

```rust
            .child(div().flex_1().min_h_0().children(self.active_view().cloned()))
```

with

```rust
            .child(div().flex_1().min_h_0().flex().child(self.layout_element(self.terminals.layout(), cx)))
```

and add to `impl TerminalPanel` (after `pub fn started`):

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
                let pane = div().flex_1().min_w_0().min_h_0().size_full().flex().gap(px(1.0)).bg(theme::color(cx, "border"));
                let pane = match split.orientation {
                    SplitOrientation::Horizontal => pane.flex_row(),
                    SplitOrientation::Vertical => pane.flex_col(),
                };
                pane.children(split.children.iter().map(|child| self.layout_element(child, cx))).into_any_element()
            }
        }
    }
```

`render` takes `cx: &mut Context<Self>`; `self.layout_element(…, cx)` passes it as `&App` by deref. If the borrow checker objects to `self.terminals.layout()` borrowed while `self.layout_element` is called (both are `&self`, so it should not), clone the layout first.

## 4. Menus

- `menu_spec.rs` already has `act("navigate.showFilePath", "File Path")` and `act("navigate.selectIn", "Select In…")`; they now run.
- `context_menus.rs` already offers `navigate.selectIn` with `{ "path" }` ("Project View") and `project.openInTerminal` ("Terminal"); both now run.
- `mod.rs` already offers `project.copyFileName` and `project.copyRelativePath` with a path argument (the editor tab menu); they now run.
- The Project tool window header (Expand All, Collapse All, the options gear) and the terminal tab context menu (Split Right, Split Down, `TerminalView.tsx:122`) are not built; nothing to add.

## 5. What each registered action calls (for the parity records)

| Action                                                                                                                 | Backend / IPC it stands for                                                                                                         |
| ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `navigate.selectInProjectView`, `project.reveal`, `navigate.selectIn`                                                  | `fs_read_dir` for the folders on the way                                                                                            |
| `navigate.showFilePath`                                                                                                | none; a chosen row reveals in the OS file manager (`cx.reveal_path`)                                                                |
| `project.expandAll`                                                                                                    | `fs_read_dir` per folder (`ide_host::ops::fs::read_dir`), at most 400                                                               |
| `project.collapseAll`, `project.option.*`, `project.sortBy.*`                                                          | none (UI storage)                                                                                                                   |
| `project.copyPath`, `project.copyFileName`, `project.copyRelativePath`, `project.copySourceRootPath`                   | `project_roots` (`ide_host::ops::project::roots`), then the clipboard                                                               |
| `project.compareWith`                                                                                                  | `fs_read_text` through `open_diff` / `load_sides`                                                                                   |
| `project.open`                                                                                                         | `doc_open` through `open_project_file`                                                                                              |
| `project.openInTerminal`, `terminal.openAt`, `terminal.newPredefinedSession`, `terminal.runCommand`, `terminal.split*` | `term_spawn` (`ide_host::ops::term::spawn`, from `TerminalView::new`)                                                               |
| `terminal.unsplit`                                                                                                     | none (a spawned shell is not touched)                                                                                               |
| `project.findInFolder`                                                                                                 | delegates to `find.inPath` (`search_start`)                                                                                         |
| Mark Directory as (context menu row)                                                                                   | `project_roots` for the detected roots, `settings_set` (scope project, `projectStructure.<list>`), then `fs_read_dir` of the parent |

## 6. Not registered, and why

Project view (`src/features/project-tree/actions.ts`):

- `project.revealInFinder`: gpui's test platform has `fn reveal_path(&self, _path: &Path) { unimplemented!() }` (`gpui-pre-0.3.12/src/platform/test/platform.rs`), so no test can run it. The context menu's Open In › Finder row already reveals through `Effect::RevealInFileManager`.
- `project.refresh`, `project.reloadFromDisk`: the file watcher re-reads the same folders, so no test can tell the action's re-read from the watcher's. `ProjectTree::reload_all` and the `reload_from_disk` flow exist; wiring is one `handle` each.
- `project.renameInline`: the tree has no inline editor.
- `project.findUsages`: needs a language server. The draft `workbench/tests/proofs_editor_search_safe_delete.rs` (`find_usages_on_a_file_lists_the_usages_of_its_symbols_from_other_files`) covers it with a fake server.
- `project.showHistory`: `vcs.localHistory` is not registered.
- `project.reformatCode`, `project.optimizeImports`: `code.reformatFile` and `code.optimizeImportsFile` are not registered.
- `project.viewMode.*` (dynamic `actions.ts:177`): `ProjectTree` has no panes.
- `project.option.{showMembers, autoscrollFromSource, autoscrollToSource, singleClickOpen, singleClickExpand, previewTab}` (dynamic `actions.ts:167`): the tree does not act on them. A toggle that changes nothing visible is not registered.
- `project.copy`, `project.cut`, `project.paste` (`~`): registered already and unchanged. A new test, `cut_and_paste_moves_the_file_and_a_copy_onto_a_taken_name_asks_for_another`, checks:
  - the files on disk after a confirmed cut and paste;
  - the "'a.txt' already exists" refusal of a copy onto a taken name;
  - the copy under the next name given.

  Still open: `project_files.rs` drops `OsClipboardWrite::Files` (`Effect::HandOff`), so other applications cannot paste the files. Hypothesis: gpui offers no file-list clipboard write. To confirm, grep `ClipboardEntry` / `write_to_clipboard` in `gpui-pre-0.3.12/src/platform/mac/`.

- `project.delete` (`~`): unchanged.
  - `ide_vfs::delete` is `ops::delete(path, !cfg!(test))`. `cfg!(test)` holds only in ide-vfs's own unit tests, so from app-gpui tests a confirmed delete reaches the real Trash.
  - `ide_host::ops::fs::delete` (IPC `fs_delete`) has no permanent flag. Grep for `permanent` in `project_view/file_dialogs.rs` and `project_view/operations.rs` finds nothing, so the Delete dialog offers no permanent choice either.
  - The existing test removes the file before confirming, so the backend refuses it with "not found" before any Trash call.
  - No new delete test is added. Proving a real delete needs a seam, such as a permanent-delete flag on `ops::fs::delete` or a `Host` option.
- `project.rename` (`~`), `services.addService` (`~`): unchanged. Their notes are about a language server's semantic update, and about the Add Service picker and menu position. Neither is in this module.
- Duplicate, New Scratch File, the group options, the view's options menu and speed search have no `project.*` ids in `actions.ts`. Duplicate is `project.copyElement`, registered already. The options menu and speed search are `HeaderActions.tsx` / `ProjectView.tsx` UI. The tree has no header and no speed search, so nothing is registered for them.
- Run All / Restart All / Stop All, Hide and Delete of a type group are `run.services.*` / `MenuCommand::{HideConfig, RemoveType}`, registered or carried out already.

Terminal (`src/features/terminal/index.ts`):

- `terminal.find`: there is no find bar view. `TerminalEvent::FindRequested` is dropped in `TerminalPanel::terminal_event`, and the emulator has no scrollback search.
- `terminal.pluginSession`: no plugin host hands terminal requests to this window. `Terminals::plugin_session` exists, but nobody reports the exit back to the plugin.
- `terminal.closeSession` (`~`): unchanged. The pieces exist: `ide_host::ops::term::is_busy` (IPC `term_is_busy`), `crate::terminal::store::terminate_confirmation` and `PromptDialog::confirm`. They were not built in this pass because the handler is registered inside `mod.rs`' `register_actions`, which another agent is editing now. Building it means replacing that handler, asking only when `is_busy` says so, and adding a test that runs `sleep 30` and answers Cancel, then Terminate.

Services (`src/features/services/`):

- `docker.*` (`start`, `stop`, `restart`, `remove`, `showDashboard`, `showLog`, `openLogInEditor`, `showProcesses`, `showLayers`, `inspect`, `execShell`, `pullImage`, `prune`) and the toolbar ids `start`, `stop`, `restart`, `delete`, `pull`, `terminal`, `refresh`:
  - `ide_docker` drives the `docker` CLI. `docker_executable()` resolves it once per process into a `static OnceLock`, from `OPENIDEA_DOCKER`, then `which docker`, then fixed install paths (`/usr/local/bin/docker`, `/opt/homebrew/bin/docker`, `~/.orbstack/bin/docker`, …).
  - A test cannot confine it to a fake script. Any earlier test in the process (Services shows Docker nodes and polls `snapshot`) may already have pinned the real binary, and setting `OPENIDEA_DOCKER` is a process-wide, racy env change. A test could then reach the user's real daemon.
  - `carry_out_docker`'s `other =>` arm also drops `ContainerOp`, `Remove`, `Stream`, `Inspect`, `ConfirmDelete`, `PromptPull` and `ConfirmPrune` today.
  - Building the operations needs a seam first: an executable override on `DockerEndpoint`, or a `Host`-level Docker path read per call. Then the requests need carrying out (IPC `docker_container_op`, `docker_remove`, `docker_logs`, `docker_inspect`, `docker_image_history`, `docker_container_top`, `docker_pull`, `docker_prune`).
- `docker.refresh` (`~`): unchanged, for the same reason.
- `docker.toggleStoppedContainers`, `docker.toggleUntaggedImages` (`~`): registered already. They only flip `DockerFilters` and store `FILTERS_KEY` (`DockerStore::carry_out`, `set_filters`): no Docker call. `services_actions.rs` now runs both through the registry and asserts the stored filters, which closes these two rows.
- The Move Up / Move Down test shows the Services tool window, which starts the Docker watch. The existing Services test (`a_run_configuration_added_to_services_runs_from_its_row_into_the_detail_pane_and_stop_ends_a_service`) does the same. If a `docker` binary is found, that watch runs only the read-only `docker version` and listings (`ide_docker::snapshot`). It never runs container operations.
- Run All / Stop All / Restart All of a type group (`run.services.*`): registered already. `workbench/tests/proofs_services.rs` (an unwired draft) runs them end to end; not duplicated.
- `moveUp`, `moveDown`: toolbar items of the view, not registry actions. `services_actions.rs` proves them end to end.

## 7. Behaviour not built

- Select In: the chooser opens near the top of the window, not at the caret. `with_menu_context` passes an empty `Contributed`, so contributed targets (Structure, …) are not listed.
- Existing tests: grep of `workbench/` for assertions on these ids or their menu rows finds only `a_tabs_menu_closes_pins_and_splits`. It asserts the tab menu's top-level rows, and "Copy Path/Reference…" is there already (from `project.copyAbsolutePath`). Its submenu now gains "File Name" and "Path From Content Root", which no test lists. `bookmarks_window.rs:67` and `mod.rs`' bookmark jump now select in the tree instead of logging "not available"; no test asserts the log. "Mark Directory as is not available here" is asserted by no test.
- File Path: rows show the folder's name only, not its full path beside it (`MenuItem` has no detail column).
- Mark Directory as: `with_menu_context` still passes `roots: &[]`, so the submenu never shows "Unmark as …" / "Cancel Exclusion" for a marked folder. Feed it the effective roots from `ide_host::ops::project::roots` to get that.
- Expand All runs from the action only. The tree's own keys (`*` on the numeric pad) are not handled.
- Terminal: per-group tab strips (the strip still lists every tab), the split handles' dragging, and the split rows of the tab menu.
- With `terminal.runCommand` registered, the `else if id == "terminal.runCommand"` fallback in `workbench/github.rs` (`PullRequestsEvent::RunAction`) is dead code, and "Log in with gh" really runs `gh auth login` in a terminal tab (the intended behaviour). No test clicks `gh-login` (grep of `workbench/`), so no test runs the real `gh`.

## 8. How the terminal tests stay away from the user's shell

Established by reading the code and `/etc/zshrc` on this machine, not by running a shell. To confirm, run a terminal test and check that `~/.zsh_history` keeps its modification time, with `stat -f %m ~/.zsh_history` before and after.

The tests start shells exactly as the existing Terminal tests do: `start(cx)` calls `crate::terminal::test_support::isolate_shells(home, cx)`.

- That sets Settings › Terminal › env (global, in the temp home) to `HOME=<tmp>/shell-home`, `ZDOTDIR=<tmp>/shell-home` (with an empty `.zshrc`) and `HISTFILE=/dev/null`.
- The default shell is the account's login shell with `-l` (`ide_terminal::resolve_shell`). zsh gets the integration's `ZDOTDIR`, with `OPENIDEA_USER_ZDOTDIR=<tmp>/shell-home`, taken from the env above (`integration::apply`).
- macOS `/etc/zshrc` sets `HISTFILE=${ZDOTDIR:-$HOME}/.zsh_history` unconditionally. Read on this machine: line 15. That points into the integration directory, and the generated `.zshrc` then moves it to `${OPENIDEA_USER_ZDOTDIR:-$HOME}/.zsh_history`, that is `<tmp>/shell-home/.zsh_history`.
- No startup file of the user's (`~/.zshrc`, `~/.zprofile`, `~/.profile`) is read, since `HOME` and `ZDOTDIR` are the temp folder. No line can reach `~/.zsh_history`: every history path the shell can compute is under `<tmp>`.
- Run Command runs `/bin/sh -c "…"` with explicit arguments: no login, no interactive startup, no history.
- New Predefined Session runs `/bin/sh -l` (bash in POSIX mode on macOS). It reads `/etc/profile` and `$HOME/.profile`, which is the temp home. Its history goes to `HISTFILE=/dev/null` from the env.

## 9. Assumptions an integrator should check first

1. Keys
   - `cx.simulate_keystrokes("cmd-=")` and `"cmd--"` reach the keymap as `Meta+=` / `Meta+-` (intellij-macos binds `project.expandAll` / `project.collapseAll` there, and `code.unfold` / `code.fold` to the same keys). The dispatcher picks the binding whose `keyWhen` holds.
   - If gpui parses `"cmd--"` differently, write `"cmd-minus"`.
   - The tests run on macOS; on Linux CI the keymap is intellij-windows (`Ctrl+=`, `Ctrl+-`), so those two key presses need `ctrl-` there (gate on `cfg!(target_os = "macos")` if CI runs Linux).
2. `layout.show(TERMINAL_TOOL_WINDOW, true)` mounts the `TerminalPanel` synchronously, so `terminal_panel()` finds it right after `with_layout`. `terminal.new` in `mod.rs` relies on the same.
3. `gen` is not one of the backend's detected excluded folder names (the excluded test expects both files to be found before the mark). If it is, rename the folder in `an_excluded_folder_shows_as_excluded_and_the_search_skips_it`.
4. `ide_host::ops::project::roots(..).roots` includes the user's marks right after `settings::set`. `Project::roots` reads `settings.effective()` per call.
5. A disabled `MenuItem::entry(title, None)` is not selectable, so the first selected row is "Project View" / the file. The File Path test expects `"> main.rs"` second.
6. `PopupMenu`'s `down` moves the selection of the deepest open level (used by `choose_in_menu` for Mark Directory as › …).
7. Toggle state is read from `UiStorage` (`ViewOptions::stored`), which `ProjectTree::set_options` writes, never from the workbench entity.

## 10. Compile risks

- `start_flow`'s `Option<Done>` with a private alias: see 2a.
- `on_backend` copies `project_files.rs`' `host_call` (including `use<T, F, W>`). If the precise-capture syntax trips, drop `+ use<…>`.
- `panel.change(cx, |terminals| open(terminals, &root, &terminal))` inside `panel.update`: `open` is `FnOnce`, moved into the inner closure.
- `services_actions.rs` holds only `#[cfg(test)] mod tests`. That is fine for `-D warnings`.

## 11. Tests most likely to fail on first run

- `expand_all_and_collapse_all_open_and_close_every_folder_and_their_keys_act_in_the_tree_only`: key spelling (9.1), and whether the editor's `code.unfold` swallows `cmd-=` before the dispatcher tries the tree's binding.
- `the_splits_lay_their_shells_side_by_side_or_stacked_and_unsplit_joins_them`: pane bounds need a paint after each split. The helper runs `run_until_parked` first; if `debug_bounds` is still empty, call `cx.update(|window, _| window.refresh())` before it.
- `the_copy_path_forms_copy_the_selection_and_the_source_root_path_follows_mark_directory_as`: the context-menu walk (9.5, 9.6), and the clipboard text of `copy_paths` (one path, no trailing newline).
- `select_in_lists_the_places_that_show_the_file_and_the_project_view_selects_it`: it expects a "Terminal" row, which appears only once `terminal.openAt` is registered (1b).
