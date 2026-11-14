# Integrating the Run context and Debugger extra actions

New files, not compiled yet:

- `app-gpui/src/debugger/evaluate_dialog.rs` (the Evaluate dialog view, its history helpers, its tests)
- `app-gpui/src/workbench/debug_extras.rs` (`debug.evaluate`, `debug.smartStepInto`, the shared end-to-end fixture `debug_extras::tests::{Fixture, open_project, long, run, context, PROGRAM}`)
- `app-gpui/src/workbench/run_context.rs` (`run.runContext`, `run.debugContext`, `run.modifyContext`, `run.chooseDebugConfig`)

Every anchor below is quoted from the file as it is now; line numbers are left out because `mod.rs` moves. All sections are required: without §4 and §5 the new files do not compile, without §2e the Evaluate dialog is never drawn, without §3a the Smart Step Into menu never opens.

## 1. `app-gpui/src/debugger/mod.rs`

After `pub mod effects;` add:

```rust
pub mod evaluate_dialog;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The modules

After `mod debug;` add:

```rust
mod debug_extras;
```

After `mod run_configs;` add:

```rust
mod run_context;
```

### 2b. The `Workbench` fields

In `pub struct Workbench`, after

```rust
    /// The debugger of the open project: its driver, the Debug tool window and the Breakpoints dialog.
    debug: Option<debug::DebugSlot>,
```

add:

```rust
    /// The Evaluate dialog, while it is open.
    debug_extras: debug_extras::DebugExtrasSlot,
    /// The tools located for context runs, and the builds a debug of a compiled file waits for.
    run_context: run_context::RunContextSlot,
```

### 2c. Their initial values

In `Workbench::new`, in `let mut workbench = Self { … }`, after `            debug: None,` add:

```rust
            debug_extras: Default::default(),
            run_context: Default::default(),
```

### 2d. The actions

In `pub fn register_actions(cx: &mut App)`, after `    debug::register_actions(registry);` add:

```rust
    debug_extras::register_actions(registry);
    run_context::register_actions(registry);
```

### 2e. The Evaluate dialog's layer

In `fn render`, after

```rust
            .children(self.debug_breakpoints_layer(cx))
```

add:

```rust
            .children(self.debug_evaluate_layer(cx))
```

It is a `deferred(..).with_priority(1)` layer like the Breakpoints dialog's, so it sits under the popup slot (the Smart Step Into menu and any other menu open over it).

## 3. `app-gpui/src/workbench/debug.rs`

### 3a. The Smart Step Into menu follows the debugger

Replace

```rust
    fn debugger_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_debug_marks(window, cx);
        self.sync_breakpoints_dialog(window, cx);
        cx.notify();
    }
```

with

```rust
    fn debugger_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_debug_marks(window, cx);
        self.sync_breakpoints_dialog(window, cx);
        self.sync_smart_step_chooser(window, cx);
        cx.notify();
    }
```

`sync_smart_step_chooser` takes `overlays.smart_step` out of the debugger (so no overlay stays open behind the menu) and shows the calls in a menu.

### 3b. The Evaluate dialog goes with the project

Replace

```rust
    pub(super) fn reset_debugger(&mut self, cx: &mut Context<Self>) {
        if let Some(slot) = self.debug.take() {
            slot.driver.update(cx, |driver, cx| driver.reset(cx));
        }
    }
```

with

```rust
    pub(super) fn reset_debugger(&mut self, cx: &mut Context<Self>) {
        self.debug_extras = Default::default();
        if let Some(slot) = self.debug.take() {
            slot.driver.update(cx, |driver, cx| driver.reset(cx));
        }
    }
```

### 3c. The doc comment of `DEBUG_ACTIONS`

Replace

```rust
/// The `debug.*` actions this window carries out on its debugger. Evaluate Expression, Attach to
/// Process, Smart Step Into, Quick Evaluate and the console history have no view here yet.
```

with

```rust
/// The `debug.*` actions this window carries out on its debugger. Evaluate Expression and Smart Step
/// Into are registered by `debug_extras`; Attach to Process, Quick Evaluate and the console history
/// have no view here yet.
```

## 4. `app-gpui/src/debugger/actions.rs`: stepping into the chosen call

`step_into` and `resume_titles` are private to `Debugger`; the menu's choice needs a public entry. Directly before

```rust
    /// Opens the Attach to Process popup and loads the process list.
    pub fn open_attach_popup(&mut self, host: &mut Host) {
```

insert:

```rust
    /// Smart Step Into's chooser picked the call `target`: `thread` of `session` steps into it; a
    /// failure is reported as Smart Step Into's.
    pub fn step_into_target(&mut self, session: DebugSessionId, thread: i64, target: i64, host: &mut Host) {
        let call = BackendCall::DebugStepInTarget { session, thread, target };
        if let Some(ticket) = self.state.resume_execution(session, call) {
            self.resume_titles.insert(ticket, host.i18n.tx("Smart Step Into failed"));
        }
    }
```

`DebugSessionId` and `BackendCall` are already imported there.

## 5. `app-gpui/src/run/driver.rs`: Modify Run Configuration…

`carry_out_persist` is private; the store's `modify_temporary` must be persisted like every other change. Directly before

```rust
    /// The edit dialog closed: a configuration it was opened to modify and that was not saved goes back
```

insert:

```rust
    /// "Modify Run Configuration…": `config` becomes the selected temporary configuration, marked to
    /// open in Edit Configurations with pending changes (OK saves it). Returns its id.
    pub fn modify_temporary(&mut self, config: RunConfig, cx: &mut Context<Self>) -> String {
        let (persist, id) = self.store.update(cx, |store, cx| {
            let persist = store.modify_temporary(config, &[]);
            cx.notify();
            (persist, store.edit_target.clone().unwrap_or_default())
        });
        self.carry_out_persist(persist, cx);
        id
    }
```

## 6. `app-gpui/src/workbench/run_configs.rs`: the project's end

Replace

```rust
    pub(super) fn drop_run_configs(&mut self) {
        self.run_widget = None;
        self.edit_configs = None;
    }
```

with

```rust
    pub(super) fn drop_run_configs(&mut self) {
        self.run_widget = None;
        self.edit_configs = None;
        self.run_context = Default::default();
    }
```

## 7. Menus

No menu entry to add. `menu_spec.rs` already lists `run.runContext`, `run.debugContext`, `run.chooseDebugConfig`, `debug.smartStepInto`, `debug.evaluate` (Run menu); `run/widget_view.rs` `more_menu` lists `run.runContext` / `run.debugContext`; `debugger/window_view.rs` `more_menu` lists `debug.smartStepInto` / `debug.evaluate`; `context_menus.rs` offers `debug.evaluate` in the editor menu while paused. They show once the ids are registered. The Run feature's group of the editor, project view and editor tab menus (`context_run::context_menu_items`, "Run '<name>'" / "Debug '<name>'" / More Run/Debug › Modify Run Configuration…) and the run gutter popup are not wired anywhere native (`grep -rn "context_menu_items\|target_menu_items" app-gpui/src --include='*.rs'` finds only `context_run.rs` and the project view's own function); `run.modifyContext` is reachable only from the registry/Find Action until they are.

## 8. IPC commands each action stands for

- `run.runContext` / `run.debugContext`: `run_configs` (through `when_loaded`), `fs_read_text` (a context without text), `sdk_locate_tools`, `sdk_python_effective`, `sdk_php_effective`, `fs_read_dir` (runtime markers), then `run_start` (Run) or `run.debugConfig` → `debug_start` (Debug); a compiled file: `run_start` of the build, then `debug.startConfig`.
- `run.modifyContext`: the same lookups, `run_without_derived` (persisting the temporary configuration), `run_templates` (Edit Configurations).
- `run.chooseDebugConfig`: `run_configs`; choosing → `run.debugConfig`.
- `debug.evaluate`: `debug_evaluate_in`.
- `debug.smartStepInto`: `debug_step_in_targets`, `debug_step_in_target`, or `debug_step` (into).

All names checked in `src-tauri/src/commands/{run,debug,fs,sdk}.rs`.

## 9. Not built, and existing behaviour that changes

- **Current File hand-over.** With these ids registered, `RunDriver::run_selected`, the chooser's Current File row (`run_configs.rs` `ChooserAction::Run if current_file`) and `debug_selected` (`debug.rs`) now reach `run.runContext` / `run.debugContext`. The context's temporary configuration then becomes the selection; the webview's `runCurrentFile` selects Current File again afterwards. Not built: the action cannot tell a hand-over from ⌃⇧R. A fix is an argument `{ "currentFile": true }` from those three callers and `select_config(Some(CURRENT_FILE))` after the adoption in `launch_production`.
- **Producers' input left at defaults** (`with_context_input`): Go module paths (`loadGoModules`), Python packages (`loadPythonPackages`), Project Structure content roots and Gradle modules, inherited mains, configuration templates, source roots, `gradle_local`. Go files in modules, Python files with relative imports and Gradle/JVM sources may produce something other than the webview's.
- **`remoteDebugBlocked`**: a debug of a configuration on a non-local run target is not refused with "Debugging on run targets is not supported"; the debugger's own start decides.
- **Build then debug** (a single Rust / C / C++ file): written (`Launch::BuildThenDebug`, `after_session_ends`), not covered by a test (no compiler in tests).
- **Evaluate dialog**: one-line field (a selection over several lines is joined with spaces), no expression completion, no History button menu (↑ / ↓ only), the result is one row and cannot be expanded.
- **Smart Step Into**: a popup menu near the top of the window instead of the "Step Into" dialog; no inline picking in the editor (the driver passes no paused line: `driver.rs` `step_in_targets_loaded(ticket, result, None, host)`).
- **Context menus and the run gutter**: see §7.

## 10. Assumptions to check first

1. `TextField`'s claimed keys: the dialog claims Enter/Escape/↑/↓ through `input::claim_keys` and ignores `TextFieldEvent::Submitted`, as `PromptDialog` does. If a claimed key also reaches the field, Escape would emit `Closed` twice; `close_evaluate_dialog` is idempotent.
2. `DebugDriver` notifies its `Debugger` entity after the step-in targets arrive (`step_in_targets_loaded` → `pump`), so `debugger_changed` runs and opens the menu. If it does not, call `sync_smart_step_chooser` from `debug_driver_event` instead.
3. The session's `supports(|caps| caps.step_in_targets)` reflects `supportsStepInTargetsRequest` from the fake adapter's `initialize` (`ide-dap/src/lib.rs` maps it; the UI reads capabilities through `DebugCapabilities`).
4. The produced configuration of the extensionless `tool` (`#!/bin/echo hello`) is named `tool`, has an id starting `context:file:`, and runs `/bin/echo hello <arg>` (`shebang_production`'s absolute-program branch).
5. `Editor::position_of` is in window coordinates (used for the chooser below the caret).
6. Closing a `Subscription` is never done from inside its callback: finished build waits are pruned when the next one is added.

7. A configuration of kind `echo` (no registered type) starts with its command and arguments (`StartRun` looks types up with `is_some_and`), and Edit Configurations lists it under its name `tool`.
8. The Debug tool window's Variables pane lists a watch as a row starting with its expression.

## 11. Compile risks

- `Option::take_if` (`evaluate_dialog.rs`) needs Rust 1.80.
- Let-chains in `run_context.rs` (`if … && let Some(editor) = shown`), as the crate already uses.
- `tool_answers` must be `Send` for `Backend::with_host`; it awaits `ide_host::ops::sdk::sdk_locate_tools` and `ide_host::ops::fs::read_dir` only.
- A few lines of the new files may pass 120 columns; they compile as they are.
