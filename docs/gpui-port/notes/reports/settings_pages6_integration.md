# Settings pages 6: Editor › Vim (whole) and Tools › Agents (not started)

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Nothing here was compiled. All new files are untracked and declared by nobody until the hunks below land.

New files:

- `app-gpui/src/settings_view/pages/editor_vim.rs` (the page view, its `VimRcStatus` global, 7 view/model tests)
- `app-gpui/src/workbench/settings_vim_page.rs` (mount, rc reading off the main thread, `vim.settings` action, `rc_home`)
- `app-gpui/src/workbench/tests/proofs35_settings_vim.rs` (10 end-to-end tests in project-less and project windows)

## (a) Hunks, Vim group (apply in this order)

Re-read each anchor before applying: `workbench/mod.rs` and `workbench/settings_window.rs` are being edited by another agent (at the time of reading, `settings_template_pages.rs` imports `super::settings_feature_pages::mount_page`, which is a private `fn`, and `mount_template_settings_pages` has no caller; my module does not depend on either).

1. `app-gpui/src/settings_view/pages/mod.rs`, after the line

```rust
pub mod editor_todo;
```

add

```rust
pub mod editor_vim;
```

2. `app-gpui/src/workbench/mod.rs`, after the line

```rust
mod settings_template_pages;
```

add

```rust
mod settings_vim_page;
```

3. `app-gpui/src/workbench/mod.rs`, in the registration block, after the line

```rust
    settings_window::register_actions(registry);
```

add

```rust
    settings_vim_page::register_actions(registry);
```

4. `app-gpui/src/workbench/settings_window.rs`, in `open_settings`, after the line

```rust
        subscriptions.extend(self.mount_feature_settings_pages(&dialog, window, cx));
```

add

```rust
        subscriptions.extend(self.mount_vim_settings_page(&dialog, window, cx));
```

5. `app-gpui/src/workbench/mod.rs`, inside `mod tests`, after the line

```rust
    mod proofs32_settings_templates_fixtures;
```

add

```rust
    mod proofs35_settings_vim;
```

No menu entry is needed: `vim.settings` is not in `menu_spec.rs` today (Tools › Vim there is `vim.toggle`, which stays unregistered, see (c)).

## (b) Parity rows

| key                                                                                                                           | expected            | proving tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | missing                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ----------------------------------------------------------------------------------------------------------------------------- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:editor.vim` (new key, named as `settings:editor.todo`; INVENTORY row at `editor.vim`, src/features/vim/index.ts:22) | partial             | `workbench::tests::proofs35_settings_vim::vim_emulation_and_each_handler_are_stored_at_once_and_cancel_keeps_them`, `workbench::tests::proofs35_settings_vim::the_conflicts_table_lists_the_ide_actions_the_keymap_binds_to_each_vim_key`, `workbench::tests::proofs35_settings_vim::the_rc_file_in_the_home_folder_is_read_with_the_files_it_sources`, `workbench::tests::proofs35_settings_vim::openideavimrc_is_read_in_place_of_ideavimrc`, `workbench::tests::proofs35_settings_vim::reload_reads_the_rc_file_again_and_showing_the_page_again_does_not`, `workbench::tests::proofs35_settings_vim::a_project_window_reads_the_rc_file_once_the_vim_page_is_shown`, `workbench::tests::proofs35_settings_vim::the_rc_file_is_looked_for_in_the_home_of_its_own`, `workbench::tests::proofs35_settings_vim::the_open_vim_page_shows_settings_stored_by_another_window`, `workbench::tests::proofs35_settings_vim::the_settings_search_finds_the_vim_page_by_a_label_on_it`; view tests `settings_view::pages::editor_vim::tests`. The checkbox, the handler choices and Reset to Defaults are changed with the mouse in a project window and read back from the UI storage; nothing is staged (Apply off, no Reset, no modified mark) and Cancel keeps the changes, as the webview applies them at once | Nothing in the native app reads the stored setting: `VimMode` (`features_a/vim/mode.rs`) is never constructed outside its tests, `vim_item`/`vim_menu` (`workbench/status_items.rs:217,228`) have no caller, no `DispatchEnv::claimed` consults `vim_claims`, `vim.toggle`/`vim.reloadRc` are not registered, and the editor feeds no keystrokes to the engine. The page stores and shows everything as the webview's does; turning Vim on changes nothing yet. |
| `action:vim.settings` (INVENTORY line `vim.settings`, src/features/vim/index.ts:107)                                          | done                | `workbench::tests::proofs35_settings_vim::vim_settings_opens_settings_on_the_vim_page`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | —                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `unit:src/features/vim/__tests__/conflicts.test.ts`, `rc.test.ts`, `vim.test.ts`                                              | unchanged           | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | No spec file tests `VimSettingsPage.tsx` or `rcFile.ts` (grep of `src` and `e2e` for `VimSettingsPage` / `editor.vim` finds only the component and `index.ts`). The page tests are named after what they prove.                                                                                                                                                                                                                                                 |
| `ipc:fs_read_text`                                                                                                            | not claimed         | the rc tests read through `ide_host::ops::fs::read_text`, the function `src-tauri/src/commands/fs.rs:97` calls, with the window's label                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | Many features use `fs_read_text`; it is not a call only this page makes.                                                                                                                                                                                                                                                                                                                                                                                        |
| `settings:tools.agents` (INVENTORY row `tools.agents`, src/features/agents/index.ts:30)                                       | not started         | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | see the Agents group                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `unit:src/features/agents/agentRegistry.test.tsx`                                                                             | unchanged (partial) | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | the view is not built                                                                                                                                                                                                                                                                                                                                                                                                                                           |

## (c) Webview behaviours of the Vim page

`src/features/vim/VimSettingsPage.tsx`:

- :38-42 Enable Vim emulation checkbox → `updateVimSettings({ enabled })`, stored at once. Built (`settings-vim-enabled`).
- :25-27 the rc file is read when the page mounts and nothing has read it in this window. Built: read when the dialog first shows the page in the app (`VimSettingsEvent::Shown`, as `ProjectTrustView` does), never while Settings shows another page; the status is an app global (`VimRcStatus`), so later showings show it until Reload.
- :44-52 "Configuration file" header with Reload → `loadRcFile()`. Built (`settings-vim-reload`). The brief's "open, reload, create" is wrong: Reload is the page's only rc action.
- :53-61 `path · N mappings`, else "No ~/.openideavimrc or ~/.ideavimrc found.". Built.
- :62-67 the "Supported: …" hint. Built, text as the JSX renders it (the line break inside `<T>` is one space).
- :68-80 the ignored lines `file:line: text — reason`. Built.
- :83-94 "Shortcut conflicts" header and hint. Built.
- :95-131 the table: Shortcut (`formatBinding`, :115), IDE action titles joined with ", " or "—" (:116), Vim's description translated (:118), Handler select Vim / IDE (:12-15, :121-126). Built; the select opens with the mouse (`settings-vim-handler-<row>`, choices `-vim` / `-ide`). Left out: the shared Select's keyboard navigation and type-ahead.
- :29-34 choosing a key's default handler removes its override. Built (`editor_vim::with_handler`).
- :133-139 Reset to Defaults, off without overrides, clears the handlers and keeps Enable. Built (`settings-vim-reset`).
- :23 rows follow the keymap and the handlers. Built (observes `input::Keymap` and `UiStorage`).
- :44, :69, :83, :125 aria-labels. Left out: no accessibility tree natively.

`src/features/vim/settings.ts`: :44-51 setState and localStorage write, built as the UI storage write in the workbench (`vim_settings_json`, same JSON shape `{"enabled":…,"handlers":{…}}`); :57-64 follows another window's write, built (`observe_global::<UiStorage>`); :21-32 parsing is the existing `parse_vim_settings`.

`src/features/vim/rcFile.ts`: :11-24 the first readable of `RC_FILES` in the home folder via `fsReadText`, built (`read_rc`); :26-44 sourced files, `~` as the home, at most 32, built through the existing `SourcedFiles`; :57-72 mappings = commands that are not `set`, built (`VimRcStatus::read`); :12 no file outside Tauri, built as "no backend or no home folder → no file".

`src/features/vim/index.ts`: :107-113 `vim.settings`, built and registered. :34-75 applying the setting (engine, rc commands, editor contribution, `vimMode` / `vim.mode` context), :79 key claim, :81-88 status item, :90-106 `vim.toggle` and `vim.reloadRc`: not built, as the brief says not to wire the engine here; this is why the page row is partial.

## (d) Hypotheses (not settled by compiling)

- Hypothesis: `ScrollEvent` scrolling of the `settings-page` scroll container works from a point over the page as in `settings_view/custom_controls.rs:1102` (my `scroll_page` copies it); settle by running `vim_emulation_and_each_handler_are_stored_at_once_and_cancel_keeps_them`.
- Hypothesis: the Vim page is taller than the 720px dialog's page area, so rows past about the tenth and Reset to Defaults are clipped; the end-to-end tests click only Escape, Ctrl+A, Ctrl+B and scroll before Reset. If the page area is taller, nothing changes.
- Hypothesis: `apply_settings` accepts a chord override `"Ctrl+Y Ctrl+Y"` (`is_valid_binding`), and the macOS/other preset does not bind `Ctrl+R` to `file.settings` (the keymap test asserts `file.settings` leaves Ctrl+R's row once the override moves). Settle by running `the_conflicts_table_lists_the_ide_actions_the_keymap_binds_to_each_vim_key`.
- Hypothesis: `cx.global_mut::<input::Keymap>()` notifies `observe_global::<Keymap>` observers (GPUI's `global_mut` pushes the notify effect, as `UiStorage` observers rely on in proofs31).
- Hypothesis: `SettingsDialog::page_id` stays the shown page while the search filters the tree, so typing in the search emits no `Shown` (read: `select_page` sets it; the filter code was not read).
- Hypothesis: in the real app, a second `Shown` while the first read is still running starts a second read (as the webview's `loaded` flag allows); harmless, the last answer wins.
- Hypothesis: the unit view tests' window (`add_window_view`, maximised test display) is tall enough for the Ctrl+R row and Reset to be clickable without scrolling.
- Hypothesis: `.map(..).unwrap_or_default()` on `Option<&[(String, String)]>` in `VimSettingsView::conflicts` compiles (`Default` for `&[T]`).
- Read, not run: `Project::text_of` (`crates/ide-host/src/project.rs:1191`) loads a path outside the project through `Documents::load_file` without a root check, so a project window reads the rc file too.

## (e) Tests most likely to fail first

- `vim_emulation_and_each_handler_are_stored_at_once_and_cancel_keeps_them`: the scroll, if the page area does not take the wheel event at the checkbox's centre.
- `the_conflicts_table_lists_the_ide_actions_the_keymap_binds_to_each_vim_key`: the preset may already bind Ctrl+R or Ctrl+Y to the chosen actions (assertions use `any`, so only the "leaves Ctrl+R" assertion could fail).
- `the_settings_search_finds_the_vim_page_by_a_label_on_it`: "Configuration file" may also match another page; the assertion only asks that Vim is among the rows.
- `editor_vim::tests::the_page_lists_every_vim_key_with_its_default_handler`: line indices assume no ignored lines and the exact line layout of `lines()`.

## (f) The real window

Showing Editor › Vim (only when the dialog shows that page) reads, through the backend's text reader, `<home>/.openideavimrc`, else `<home>/.ideavimrc`, then up to 32 files its `source` lines name (`~` is the home; other paths as written, relative ones against the process's working directory). `<home>` is `dirs::home_dir()`, or under `OPENIDEA_GPUI_HOME` / tests the folder that holds the backend's `data` folder. It writes nothing but the UI storage key `openidea.vim.v1`, opens no network connection, runs no program and holds no secret. A look at the real window should check the table's column widths with long action titles, the drop-down over the rows below, and the clipped bottom rows scrolling.

## (g) To share once all compile

- The page-shown seam (`page_shown` + `follow_page` + `Shown`) is now in `security_project_trust.rs` and `editor_vim.rs`; it belongs on `Binding` or the dialog (an event "page shown" the workbench subscribes to once), which the SSH and Deployment pages need too for anything they read lazily.
- `mount_page` (private in `settings_feature_pages.rs`, imported by `settings_template_pages.rs`) should be `pub(super)` and used by `settings_vim_page.rs` in place of its two inline `set_page_view` / `set_page_labels` lines.
- Test helpers duplicated between proofs31 and proofs35 (`open_page`, `assert_nothing_staged`, `stored_json`, `scroll_page`) belong in one settings test-support module beside `proofs32_settings_templates_fixtures.rs`.
- A machine-source stand-in pattern (a test `Global` checked before the backend call, refusing when absent) is what the SSH page's Test Connection, the Deployment page and a future Agents page would share; Vim needs none because its only source is a file under the backend's own home in tests.

# Agents group: not started

Nothing was written for `tools.agents`. Reading showed the page is too large to finish whole here: `AgentRegistryPage.tsx` (342 lines) has two tabs; the Built-in tab is `BuiltinAgents.tsx` (244 lines: launch commands and environment staged into the dialog, Browse, login hints), the ACP Registry tab a searchable list with Install / Update / Remove, a no-checksum confirmation, streamed install progress with Cancel, a 1.5 s poll of installs started elsewhere, per-row errors and external links; `agentRegistry.test.tsx` has 26 cases. The models exist (`app-gpui/src/agents/registry.rs` `RegistryPage`, effects-based; `app-gpui/src/agents/builtin_agents.rs` `BuiltinAgents`), so the view is a fresh task.

What it would touch in the real app (read, not built):

- Built-in tab: its launch commands and environment are "staged independently until Apply" (`src/features/agents/BuiltinAgents.tsx:1`), written through `agent_set_command` / `agent_set_env` (`registry.ts:49,52`); it lists `agent_providers` when it mounts (`BuiltinAgents.tsx:32-34`).
- Registry tab: `agent_registry_list` → `ide_host::ops::agent::registry_list` → `ide_agent::registry::list`, which downloads `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json` (`crates/ide-agent/src/registry.rs:26,383`) and, when that fails, reads `RegistryPaths::cache_file` (`registry.rs:285-297`); it filters by what can run here, with `on_path("npm")` among the checks (`registry.rs:224`; Hypothesis: `on_path` scans PATH and runs nothing). In the webview the fetch starts when the ACP Registry tab mounts (`AgentRegistryPage.tsx:93-104`), not when the page opens (the Built-in tab is the default, :62), and repeats every 1.5 s while an install is running (:108-132).
- Install / Update: `agent_registry_install` → `registry_install` reports progress to every project window as a background task (read, `ops/agent.rs:102-117`). Hypothesis (the rest of `registry_install` and the installer were not read): it downloads the binary or installs the npm / uv package, which may run `npm` or `uv`. Remove: `agent_registry_remove` deletes its agent file and installed files.
- `agent_providers` → `ops::agent::providers` requires a trusted project window and reads the project's agents and the registry's installed versions (`ops/agent.rs:22-27`). Hypothesis: building the provider list detects agent CLIs on the PATH; whether that runs any program was not read.
- The native guard to reuse: `workbench/agents.rs:157` `spawn_guard` (`AgentSpawnSeam`) for anything that starts an agent; a test-only registry source and installer stand-in would be needed in the same style, with one test asserting a test build without it reaches nothing.
