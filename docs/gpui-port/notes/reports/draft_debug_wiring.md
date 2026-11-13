I've written both files and the integration notes, but none of it has been compiled: no cargo, rustc or rustfmt was run. I checked bracket balance and the 120-column limit by script; no other file was edited, and `breakpoints_view.rs` is used unchanged.

## 1. Files

**`app-gpui/src/debugger/driver.rs`: `DebugDriver`**

The entity owns `Entity<Debugger>`.

- **Public methods:** `new`, `with_save_all`, `debugger`, `root`, `with_host`, `change`, `set_window_focused`, `load(root)`, `reset`, `run_action`, `notification_command`, `gutter_press`, `gutter_click` and `backend_event`.
- **Requests for the Variables view:** `scopes`, `children`, `evaluate`, `data_breakpoint_info`, `copy_value` (walks the children off the main thread) and `set_value`. Set Value's answer comes back as an event keyed by the view's request id.
- **How every change runs:** `with_host` builds the model's `Host`/`Env` from the settings in effect, the clock, trust and the text catalog. `change` then carries out the resulting effects in a loop.
- **Backend calls:** every `BackendCall` goes to `ide_host::ops::{debug, run, fs, nav, java, sdk}` off the main thread. Each answer goes to the continuation the model names, as the test benches in `actions.rs` and `state.rs` route them:
  - the breakpoint sync goes to `breakpoints_synced`, which unblocks the debug start;
  - method breakpoints and watchpoints are sent as one pair and answered through `special.synced`;
  - the remaining routes cover breakpoint states, exception filters, languages, the batched file reads and searches, and save-then-continue.
- **Storage:** writes go to `UiStorage`. `load` reads breakpoints, exception choices, special breakpoints, watches and the known exception filters.
- **Timers:** two wake-ups, one for the breakpoint sync deadline and one for queued console output.
- **Events:** `IdeEvent::Debug` goes to `handle_debug_event`, and hot code replace events go to `on_hot_code_replace`.
- **What it reports to the window (`DebugDriverEvent`):** `Notify`, `ExpireNotification`, `ShowToolWindow`, `HideToolWindow`, `RunAction`, `OpenFile`, `StatusInfo`, `FocusAppWindow` and `ValueSet`.
- **Unit tests** (real backend, no adapter):
  - a toggled breakpoint is stored, synced, read back by a new driver, then removed;
  - Mute is stored and read back;
  - a start with a missing adapter (`bash-debug`) notifies "Bash Debug is not installed" with "Install and Debug", which then runs `tools.install`.

**`app-gpui/src/workbench/debug.rs`**

- **`DebugSlot`:** the driver, the Debug tool window (view plus panes), the Breakpoints dialog and the gutter subscriptions.
- **Lifecycle:** `mount_debugger` and `reset_debugger`. `debug_backend_event` and `debug_notification_command` pass events and notification buttons to the driver. `publish_debug_context` publishes the session keys plus `debugFramesFocus` and `debugVariablesFocus`.
- **Debug tool window:** `mount_debug_window` builds `DebugWindowView` with a read-only `DebugConsolePane` showing the active session's `state.console(id)`.
  - Every event of the window, Frames and Variables views is carried out as the report's table says.
  - Closing a live session's tab asks first.
- **Editors:**
  - `watch_debug_editor` turns a gutter press into `gutter_press` / `gutter_click`.
  - `sync_debug_marks` draws breakpoints (from `build_markers`) and the execution line on two new mark layers.
- **Breakpoints dialog:** `sync_breakpoints_dialog` opens it when `overlays.breakpoints` is set; the dialog's events are carried out and `debug_breakpoints_layer` draws it.
- **Actions:** 28 `debug.*` actions:
  - `startConfig`, `stop`, `rerun`, `resume`, `pause` and `showToolWindow`;
  - `stepOver`, `stepInto`, `stepOut`, `forceStepOver` and `forceStepInto`;
  - `runToCursor`, `forceRunToCursor`, `showExecutionPoint` and `resetFrame`;
  - `setValue` and `jumpToSource`;
  - `hotReload`, `hotRestart`, `reloadChangedClasses` and `cancelHotSwap`;
  - `toggleBreakpoint`, `toggleTemporaryBreakpoint`, `toggleMethodBreakpoint`, `toggleFieldWatchpoint`, `viewBreakpoints`, `removeAllBreakpoints` and `muteBreakpoints` (as a toggle).
- **Run entries:** `run.debug` and `run.debugConfig` hand the configuration to `debug.startConfig`. Several debuggable configurations open the run chooser in Debug mode.

## 2. Drafts file

`/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/debug_integration.md`

**Test path (i).** A user language file can register a `python3` script as the adapter for a new id, `fake-dap`, because `find_config` reads user language files first. `launch_config_for` has a generic branch for an unknown adapter. The tests need `python3` on `PATH`, as the fake language server tests do.

- **§1:** the two `pub mod` lines for `debugger/mod.rs`.
- **§2: 13 edits to `workbench/mod.rs`:**
  - the `mod debug;` line and the `DEBUG_TOOL_WINDOW` import;
  - `TOOL_WINDOWS` goes from 15 to 16 (re-counted; the report said 8);
  - the `debug` field and its `None` initialiser, `register_actions`, and the mount arm;
  - the `IdeEvent::Debug` arm, `publish_context`, and the reset in `reset_runs`;
  - the mount in `open_project`, the editor hook in `load_tab`, the notification-command hook, and the dialog layer.
- **§3: `editor/marks.rs`:** layers `Breakpoints` and `Execution`; kinds `BreakpointLine`, `InactiveBreakpoint` and `ExecutionLine`, with `is_line` and `is_breakpoint`.
- **§4: `editor/view.rs` and `intelligence.rs`:**
  - a new `EditorEvent::GutterPressed` and a `gutter_line_at` check at the top of `mouse_down`;
  - three colour arms, the background-filter change, the row tint and the gutter dot;
  - the extra arm for the exhaustive match in `intelligence.rs`.
- **§5: four end-to-end tests to paste into `workbench/debug.rs`,** including the adapter script as a Rust constant:
  - `toggling_a_breakpoint_at_a_line_marks_it_persists_it_and_lists_it_in_the_breakpoints_dialog` (also checks Mute);
  - `clicking_the_gutter_toggles_the_lines_breakpoint`;
  - `debugging_a_configuration_stops_at_the_breakpoint_steps_over_and_resumes_to_the_end`;
  - `stop_ends_a_running_debug_session`.
- **§6:** each unconfirmed API with the file to check.
- **§7:** what is not wired.

## 3. Not wired

- **No view exists for these,** so their actions are not registered: Evaluate, Attach to Process, Smart Step Into picking, Quick Evaluate's hint, the breakpoint popover, and the console input and history.
- **Parts of `run.debugConfig`:** compound configurations, before-launch tasks and the Maven/Gradle agent are not built. `run.debugContext` has no native handler.
- **Plugin debug runs:** `plugin_host` is `pub(crate)` in `ide-host`, so a plugin start is answered with an error.
- **No shell or window hooks:**
  - The taskbar flash is unsupported.
  - Nothing calls `set_window_focused`.
  - `adapter_installed` is never called after an install, so "Install and Debug" does not repeat the launch.
- **Breakpoints don't follow edits** (`document_changed` isn't called).
- **Gutter:** middle click, right click and drag are not handled.
- **Not drawn or not used:** inline values, the Run to Cursor inlay, value tooltips, and the declaration search for Jump to Source of a local.
- **Context key:** `debugHotSwapBuilding` is not published, so Cancel Hot Swap Build stays disabled.
- **Debug sessions are absent from** Services, Update application and Stop's menu.

## 4. Most likely to be wrong or not to compile

1. **`with_host` in `driver.rs`:** `update_global::<Localization>` on `Context<DebugDriver>`, with `debugger.update` inside it.
2. **Closure inference in `driver.rs`:** closures typed with `Host` (a struct with a hidden lifetime) in `Fn` sugar passed to `on_debugger`; the closures passed to `ask` and `fire`; and the inner `Ok` type of the `async` blocks.
3. **`copy_value`:** it relies on `Handle::current()` and `block_on` inside `host.blocking`.
4. **Exact types of backend calls:** `java_resolve_launch` and `read_dir` return values, `run_dotnet_resolve` being re-exported, and `LanguageId::as_str`.
5. **`debug.rs`:**
   - `window.prompt(…)` and its `Receiver` compared with `Ok(0)`;
   - the `observe_in` / `subscribe_in` closures;
   - `open_debug_file` handing the keyboard back after a reveal without focus.
6. **End-to-end tests:**
   - The gutter click relies on 12 px left of the text being inside the gutter, and on the fields of `MouseDownEvent`.
   - `when` clauses depend on context keys published at draw.
   - The variable row is assumed to look like `line = {int} 2`.
   - The adapter answers `launch` only after `configurationDone`.
7. **Unchecked:** the compile risks already listed in `breakpoints_view.rs`'s own report.
