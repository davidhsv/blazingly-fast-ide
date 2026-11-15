# Plumbing rows: 32 shell IPC commands, 15 "webview plumbing" rows

Data: `plumb/rows.json`, checked by `plumb/validate.py`.

**Rule for mixed rows.** A command is classified by what it delivers itself. A part that exists only inside a feature other rows track goes to those rows (counting a tab group as one window goes to `window_group_*`, which are the groups feature). A row whose own user-visible part differs or is unmeasured is not proven.

## Counts

| Group | Class                             | Rows |
| ----- | --------------------------------- | ---- |
| 1     | behaviour-proven                  | 5    |
| 1     | behaviour-untested                | 3    |
| 1     | behaviour-missing                 | 23   |
| 1     | no-user-behaviour                 | 1    |
| 2     | every open case webview-mechanics | 7    |
| 2     | at least one user-visible case    | 8    |

## Group 1

### behaviour-proven

- `ipc:app_build_info` → `workbench::app_actions::tests::about_shows_the_version_of_the_build` and `app_views::about_dialog::tests::names_the_version_of_the_build_and_lists_its_facts`. About shows the version and the build line. Updates are hidden in both apps.
- `ipc:app_cancel_quit` → `workbench::tests::quit_from_a_window_asks_about_its_unsaved_changes_and_cancel_keeps_it`. After a Cancel, the second Quit asks Confirm Exit again, so the quit was called off.
- `ipc:window_close_context` → `proofs13_close_guard::does_not_confirm_exit_for_one_of_several_windows_or_during_quit`, `proofs13_close_guard::confirms_exit_for_the_last_window_when_enabled` and `closing_asks_about_unsaved_changes_whatever_autosave_says`.
- `ipc:window_open_files` → `proofs3_windows::open_file_opens_the_picked_file_in_the_project_or_in_the_window_that_has_it`, `proofs13_windows::leaves_files_to_the_existing_light_edit_window_or_their_projects_window` and `proofs13_windows::turns_a_window_without_a_project_into_light_edit_when_no_light_edit_window_exists`.
- `ipc:window_toggle_fullscreen` → `proofs15_full_screen::toggle_full_screen_puts_the_window_in_full_screen_and_takes_it_out`.

### behaviour-untested

One real-window run covers both:

- `user_font_data`: does a font that exists only in `~/Library/Fonts` draw? This machine has BebasNeue there. The native listing is CoreText's all-families collection. That was read in source, not measured.
- `app_info`: the About config folder is proven. The ConPTY build number (Windows) and the Wayland resize handles were never exercised, because nothing runs off macOS.

A hit-test test:

- `window_start_drag`: the title bar is a `WindowControlArea::Drag` region. The test would assert Drag on the empty space and not on a widget.

### behaviour-missing, by feature

- **Window tab groups (9):** `window_group_current`, `_select`, `_close`, `_close_tabs`, `_merge`, `_detach`, `_reorder`, `_cancel_close`, `_restore`.
  - No group action is registered, and `platform/window_groups.rs` has no caller.
  - `_restore`: natively only the last project reopens.
  - The single-window effect of `_cancel_close` is proven.
- **Title bar and window chrome (3):**
  - `window_is_fullscreen`: `TitleBarFrame.fullscreen` is never set, so the 82 px inset stays in full screen.
  - `window_set_titlebar_height`: the traffic lights are fixed at (13,13). Hypothesis: they are off-centre at zoom 150 % and in the compact header; check in the real window.
  - `window_open_projects`: the project widget's menu does not list the projects open in other windows.
- **Remote Development (2):** `remote_invoke`, `window_open_remote`. Opening a remote project answers "is on another machine".
- **WSL (1):** `wsl_resolve`. ResolveWsl is always answered `None`.
- **Comparison window (1):** `window_open_comparison`. `compare_with_branch_diffs_the_file_against_the_branch_chosen` asserts that a folder comparison opens nothing.
- **Deployment window (1):** `window_open_deployment` (model only).
- **Settings as its own window (1):** `window_open_settings`. The page and scope are proven by `settings_for_new_projects_and_plugins_open_on_their_pages`, but natively Settings is a modal layer, not a separate window. No decision accepts that difference.
- **CLI launch and hand-off (1):** `window_take_launch`. `main.rs` reads one path: `--line/--column`, `openidea diff`, several files and the hand-off to a running instance are missing.
- **Detach editor tab (1):** `window_open_file_in_new_window`. Neither the tab's "Open in New Window" nor a drop outside exists.
- **Debugger attention (1):** `window_flash_taskbar`. `driver.rs` treats the flash request as failed, so there is never a dock bounce.
- **Agents image paste (1):** `clipboard_image`. The conversation composer cannot take an image. Only the undeclared `new_task_view.rs` can.
- **Log file (1):** `log_frontend`. Logs go to stderr only: there is no file, `help.showLog` is not registered, and `error_log_sink.rs` is undeclared.

### no-user-behaviour

- `ipc:window_complete_launch` releases the shell's reservation once the webview has taken its launch; a native window opens its Target in process. Low confidence (duplicate open during opening not checked).

## Group 2

### Mechanics only, with the visible counterpart proven: record with the reason

- `module:src/platform/candidateFonts.ts`: canvas probing is a recovery path for a webview with no font catalog. The native read cannot fail.
- `unit:src/features/plugins/__tests__/workspaceContains.test.ts`: the open case is disposing a JS IPC listener.
- `unit:src/features/settings/__tests__/contrast.test.ts`: a data lint over the shared `tokens.css`, which is D102's `ideaParity` reason. `parity.json` says done while INVENTORY shows `[m]`.

### Mechanics only, but the visible counterpart is unproven: hold

- `fonts.ts` and `fonts.test.ts`: the CSS family strings and FontFace loading are mechanics, and the drawn order is proven. Drawing user-domain fonts depends on the unmeasured CoreText read (see `user_font_data`).
- `keptTabs.test.tsx`: the open cases are React mounting and a DOM budget. No window test shows the same view after switching back.
- `selectableText.test.ts`: the open cases are a class name, Text-node targets and `execCommand`. `copy_menu_for` is reached by nothing in the app.

### At least one user-visible case

- `debugger/__tests__/editor.test.ts`: the click on another gutter's empty space is **already proven** by `workbench::plugin_contributions::tests::a_gutter_icon_click_neither_moves_the_caret_nor_toggles_a_breakpoint` (body read against the spec). The other case is mechanics, so citing the test lets the row be recorded.
- `cssColor.ts`: the stricter, clamping `rgb()` parsing (D102).
- `systemFonts.ts`: the families and monospaced marks come from GPUI, not `ide_fonts::families`.
- `Tooltip.tsx`: placement beside a stripe and the flip near the window edge differ, and no test covers them.
- `terminal/embedded.test.ts`: input-method text in a Run console. Hypothesis: `RunView` has no input handler.
- `icls.test.ts`: the lenient XML parser accepts files the webview refuses, and the settings and menu round trip is not shown.
- `proxySettings.test.ts`: the problem messages. No proxy page is built, and the backend's wording differs.
- `workbench/focus.test.ts`: a popup that handed the focus elsewhere. Only a model test covers it, on the unwired `FocusTracker`.
