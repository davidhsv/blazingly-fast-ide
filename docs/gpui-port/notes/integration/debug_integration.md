# Debugging: integration instructions

Nothing here has been compiled. New files: `app-gpui/src/debugger/driver.rs` (`DebugDriver`) and `app-gpui/src/workbench/debug.rs` (the workbench side). The uncompiled `app-gpui/src/debugger/breakpoints_view.rs` is used unchanged. Every edit below is to a file that compiles today; apply them in this order and anchor on the quoted text, not on line numbers (`workbench/mod.rs` is being edited by other work).

**Test path: (i), a fake adapter.** `ide_dap::adapter::find_config` takes a user language's `[debugger]` table first. A user language file in `<config>/languages/` (read by `Backend::init` through `LanguageRegistry::load(Some(&dirs.config.join("languages")))`) can therefore name a `python3` script as the adapter for a new adapter id (`fake-dap`). `debugger::launch::launch_config_for` has a generic branch for an adapter it does not know (`program` = the command, `args` = the arguments). So a `program` configuration with `debugAdapter: "fake-dap"` launches the script over stdio, exactly as `editor/intelligence.rs::fake_server` does for a language server. The script ships as a Rust string constant (below) and exits when its input closes. The tests need `python3` on `PATH`, as the fake language server tests do.

## 1. `pub mod` lines

`app-gpui/src/debugger/mod.rs`: add, in alphabetical order with the others:

```rust
pub mod breakpoints_view;
pub mod driver;
```

## 2. `app-gpui/src/workbench/mod.rs`

2.1. Module line, next to `mod commit;`:

```rust
mod debug;
```

2.2. Import, next to `use crate::build_window::model::BUILD_TOOL_WINDOW;`:

```rust
use crate::debugger::effects::DEBUG_TOOL_WINDOW;
```

2.3. `TOOL_WINDOWS`: today it is `[&str; 15]`. It becomes `[&str; 16]` with `DEBUG_TOOL_WINDOW` added at the end (re-count if other work changed it meanwhile):

```rust
const TOOL_WINDOWS: [&str; 16] = [
    PROJECT_TOOL_WINDOW,
    COMMIT_TOOL_WINDOW,
    TERMINAL_TOOL_WINDOW,
    GIT_TOOL_WINDOW,
    FIND_TOOL_WINDOW,
    NOTIFICATIONS_TOOL_WINDOW,
    RUN_TOOL_WINDOW,
    SERVICES_TOOL_WINDOW,
    BOOKMARKS_TOOL_WINDOW,
    STRUCTURE_TOOL_WINDOW,
    TODO_TOOL_WINDOW,
    BUILD_TOOL_WINDOW,
    PROBLEMS_TOOL_WINDOW,
    USAGES_TOOL_WINDOW,
    HIERARCHY_TOOL_WINDOW,
    DEBUG_TOOL_WINDOW,
];
```

`tool_window_declaration("debug")` resolves from `contributions.json` (title "Debug", icon `toolwindow-debug`, bottom, stripe onUse, order 20). That gives `toolwindow.debug` too.

2.4. Field of `pub struct Workbench`, after `code_insight: code_insight::CodeInsight,`:

```rust
    /// The debugger of the open project: its driver, the Debug tool window and the Breakpoints dialog.
    debug: Option<debug::DebugSlot>,
```

And in `Workbench::new`'s struct literal, after `code_insight,`:

```rust
            debug: None,
```

2.5. `register_actions(cx)`, after `run_configs::register_actions(registry);`:

```rust
    debug::register_actions(registry);
```

This registers `run.debug` and `run.debugConfig`, so the run widget, the chooser, Edit Configurations and the Run view offer Debug: they read `registered("run.debug")` / `registered("run.debugConfig")`.

2.6. `mount_panels`, a new arm beside `SERVICES_TOOL_WINDOW => match self.mount_services(window, cx) {`:

```rust
                DEBUG_TOOL_WINDOW => match self.mount_debug_window(window, cx) {
                    Some(panel) => panel,
                    None => continue,
                },
```

2.7. `backend_event`, a new arm just before `IdeEvent::RunExited { .. } => {`:

```rust
            IdeEvent::Debug { .. } => {
                self.debug_backend_event(&event.event, cx);
                return;
            }
```

2.8. `publish_context`, after `self.publish_run_context(window, cx);`:

```rust
        self.publish_debug_context(window, cx);
```

This publishes `debugSessionActive`, `debugSessionRunning`, `debugSessionPaused`, `debugSessionDart`, `debugCanRerun`, `debugCanHotSwap`, `debugFramesFocus` and `debugVariablesFocus`. The workbench observes the debugger entity, so every change re-renders the window, and the keys are published again.

2.9. `reset_runs`: add at its end, so `close_project` and `open_project` both drop the debugger and stop its sessions:

```rust
        self.reset_debugger(cx);
```

2.10. `open_project`, in the `Ok((root, name)) =>` branch, right after `let run_events = cx.subscribe_in(&run, window, Self::run_driver_event);` (before `root` moves into `ProjectTree::new`):

```rust
                        workbench.mount_debugger(&root.to_string_lossy(), window, cx);
```

2.11. `load_tab`, the editor of a project tab:

- Replace `editors.insert(tab_id.clone(), editor);` with:

  ```rust
  editors.insert(tab_id.clone(), editor.clone());
  ```

- Right after `workbench.serve_reveal(cx);` in the same `Ok((info, version, rope)) =>` arm, add:

  ```rust
                        if still_open {
                            workbench.watch_debug_editor(&editor, window, cx);
                        }
  ```

2.12. `run_notification_action`, in the last arm `EntryAction::Own(own) => {`, as its first statement. Notification buttons such as "Install and Debug", "Open PHP Settings", "Cancel" of the JVM listener and the hot swap buttons then reach the debugger:

```rust
                if self.debug_notification_command(&own.command, cx) {
                    return;
                }
```

2.13. `render`, after `.children(self.edit_configs_layer())`:

```rust
            .children(self.debug_breakpoints_layer())
```

## 3. `app-gpui/src/editor/marks.rs`

3.1. `MarkLayer`, after `Diagnostics,`:

```rust
    /// The debugger's breakpoints on the document's lines.
    Breakpoints,
    /// The line where the debugger is suspended.
    Execution,
```

3.2. `MarkKind`, after `WeakProblem,`:

```rust
    /// A line with an active breakpoint: its row is tinted and a dot is drawn in the gutter.
    BreakpointLine,
    /// A line with a disabled or muted breakpoint: only a hollow dot in the gutter.
    InactiveBreakpoint,
    /// The line where the debugger is suspended: its row is tinted.
    ExecutionLine,
```

3.3. `impl MarkKind`, after `recolors_text`:

```rust
    /// Tints the whole row of its line instead of the text.
    pub fn is_line(self) -> bool {
        matches!(self, Self::BreakpointLine | Self::ExecutionLine)
    }

    /// Draws a breakpoint dot in the gutter of its line.
    pub fn is_breakpoint(self) -> bool {
        matches!(self, Self::BreakpointLine | Self::InactiveBreakpoint)
    }
```

A line mark covers the line's text (`buffer::line_range`). `set_marks` drops empty ranges, and breakpoints only go on lines with code. The execution line on an empty line is not drawn.

## 4. `app-gpui/src/editor/view.rs`

4.1. `EditorEvent`, a new variant after `Hovered { … },`:

```rust
    /// The gutter left of the text was pressed on the 0-based `line`, with Shift or Alt held.
    GutterPressed { line: u32, shift: bool, alt: bool },
```

4.2. `Editor::mouse_down`, right after `window.focus(&self.focus, cx);` (before the pointer handler). Then a press in the gutter neither moves the caret nor reaches the intelligence's pointer handler:

```rust
        if let Some(line) = self.gutter_line_at(event.position) {
            (self.drag_anchor, self.adding) = (None, None);
            cx.emit(EditorEvent::GutterPressed { line, shift: event.modifiers.shift, alt: event.modifiers.alt });
            return;
        }
```

And a new method in the same `impl Editor` block, after `mouse_down`:

```rust
    /// The 0-based line whose gutter (left of the text) is under `position`.
    fn gutter_line_at(&self, position: Point<Pixels>) -> Option<u32> {
        let layout = self.layout.as_ref()?;
        let x = position.x - layout.bounds.left();
        let y = position.y - layout.bounds.top() + self.scroll_top;
        if x < px(0.0) || x >= layout.text_left || y < px(0.0) {
            return None;
        }
        let line = (y / layout.line_height) as u32;
        (line < self.line_count()).then_some(line)
    }
```

4.3. `EditorElement::paint`, the `mark_color` closure: add three arms after `MarkKind::WeakProblem => …,`:

```rust
            MarkKind::BreakpointLine => theme::color(cx, "bg-breakpoint-line"),
            MarkKind::InactiveBreakpoint => theme::color(cx, "fg-gutter"),
            MarkKind::ExecutionLine => theme::color(cx, "bg-debug-line"),
```

4.4. Same function, the `backgrounds` closure: it must skip the debugger's marks. Replace its filter body with:

```rust
                        mark.kind != MarkKind::ExcludedOccurrence
                            && !mark.kind.is_underline()
                            && !mark.kind.is_line()
                            && !mark.kind.is_breakpoint()
                            && mark.kind.is_below_selection() == below
```

4.5. Same function, right after the `if focused && caret_here { … }` block (the current-line paint). The breakpoint row is tinted, and the execution line goes over it:

```rust
                for kind in [MarkKind::BreakpointLine, MarkKind::ExecutionLine] {
                    if marks.iter().any(|mark| mark.kind == kind && touches(mark.range.start, mark.range.end)) {
                        let row = Bounds::new(
                            point(bounds.left() + *gutter, top),
                            size(bounds.size.width - *gutter, line_height),
                        );
                        window.paint_quad(fill(row, mark_color(cx, kind)));
                    }
                }
```

4.6. Same function, right after the line-number paint (`if let Some(number) = numbers.get(index) { … }`). This draws the breakpoint dot at the gutter's left edge:

```rust
                let breakpoint =
                    marks.iter().find(|mark| mark.kind.is_breakpoint() && touches(mark.range.start, mark.range.end));
                if let Some(mark) = breakpoint {
                    let diameter = px(10.0);
                    let dot = Bounds::new(
                        point(bounds.left() + px(4.0), top + (line_height - diameter) / 2.0),
                        size(diameter, diameter),
                    );
                    if mark.kind == MarkKind::BreakpointLine {
                        let danger = theme::color(cx, "danger");
                        let solid = gpui_kit::BorderStyle::Solid;
                        window.paint_quad(quad(dot, diameter / 2.0, danger, px(0.0), danger, solid));
                    } else {
                        let ring = mark_color(cx, mark.kind);
                        let hollow = ring.opacity(0.0);
                        let solid = gpui_kit::BorderStyle::Solid;
                        window.paint_quad(quad(dot, diameter / 2.0, hollow, px(1.5), ring, solid));
                    }
                }
```

4.7. `EditorEvent` is matched exhaustively in `app-gpui/src/editor/intelligence.rs` (`fn editor_event`). Add an arm after `&EditorEvent::Hovered { … } => …,`:

```rust
            EditorEvent::GutterPressed { .. } => {}
```

Also grep `EditorEvent::` across `app-gpui/src` for any other exhaustive `match`; today only `intelligence.rs` has one.

## 5. End-to-end tests (path i)

Paste this block at the end of `app-gpui/src/workbench/debug.rs`. They need the edits of §1–§4.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AppDirs, Backend};
    use crate::debugger::breakpoints::store::storage_key;
    use crate::debugger::breakpoints_view::DialogRow;
    use crate::debugger::state::SessionStatus;
    use crate::platform::actions::execute_action_or_throw;
    use crate::platform::context::window_context;
    use crate::platform::input;
    use crate::platform::storage::UiStorage;
    use crate::run::configs::blank_config;
    use gpui_kit::{Modifiers, MouseButton, MouseDownEvent, TestAppContext, VisualTestContext, point};
    use std::time::Duration;

    /// A debug adapter speaking DAP over stdio. It runs the launched `program` file line by line: it
    /// stops at a line with a breakpoint, steps one line on `next`, runs to the next breakpoint on
    /// `continue` and ends the program after its last line. A line that reads `loop` keeps the
    /// program running until it is stopped. The only frame is `main` at the current line, with one
    /// local, `line`, holding the 1-based line. It exits when its input closes.
    const FAKE_ADAPTER: &str = r#"import json, os, sys
out = sys.stdout.buffer
seq = 0
def send(message):
    global seq
    seq += 1
    message['seq'] = seq
    body = json.dumps(message).encode()
    out.write(b'Content-Length: %d\r\n\r\n' % len(body) + body)
    out.flush()
def respond(request, body=None):
    send({'type': 'response', 'request_seq': request['seq'], 'command': request['command'],
          'success': True, 'body': body or {}})
def event(name, body=None):
    send({'type': 'event', 'event': name, 'body': body or {}})
def read():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            if length is not None:
                return json.loads(sys.stdin.buffer.read(length))
            continue
        key, _, value = line.decode().partition(':')
        if key.strip().lower() == 'content-length':
            length = int(value.strip())
source, lines, breakpoints, current, launch, ended = None, [], set(), 0, None, False
def stop(reason):
    event('stopped', {'reason': reason, 'threadId': 1, 'allThreadsStopped': True})
def finish():
    global ended, current
    if not ended:
        ended = True
        current = 0
        event('exited', {'exitCode': 0})
        event('terminated')
def run(start):
    global current
    for number in range(start, len(lines) + 1):
        current = number
        if number in breakpoints:
            stop('breakpoint')
            return
        if lines[number - 1].strip() == 'loop':
            return
    finish()
while True:
    request = read()
    if request is None:
        sys.exit(0)
    if request.get('type') != 'request':
        continue
    command = request['command']
    arguments = request.get('arguments') or {}
    if command == 'initialize':
        respond(request, {'supportsConfigurationDoneRequest': True})
    elif command == 'launch':
        launch = request
        source = os.path.join(arguments.get('cwd') or '.', arguments['program'])
        with open(source) as program:
            lines = program.read().splitlines()
        event('initialized')
    elif command == 'setBreakpoints':
        wanted = [b['line'] for b in arguments.get('breakpoints', [])]
        breakpoints = set(wanted)
        respond(request, {'breakpoints': [{'id': i + 1, 'verified': True, 'line': l} for i, l in enumerate(wanted)]})
    elif command == 'configurationDone':
        respond(request)
        if launch is not None:
            respond(launch)
            launch = None
        run(1)
    elif command == 'threads':
        respond(request, {'threads': [{'id': 1, 'name': 'main'}]})
    elif command == 'stackTrace':
        frames = []
        if current and not ended:
            frames = [{'id': 1000 + current, 'name': 'main', 'line': current, 'column': 1,
                       'source': {'name': os.path.basename(source), 'path': source}}]
        respond(request, {'stackFrames': frames, 'totalFrames': len(frames)})
    elif command == 'scopes':
        respond(request, {'scopes': [{'name': 'Locals', 'variablesReference': 1, 'expensive': False}]})
    elif command == 'variables':
        respond(request, {'variables': [{'name': 'line', 'value': str(current), 'type': 'int',
                                         'variablesReference': 0}]})
    elif command == 'continue':
        respond(request, {'allThreadsContinued': True})
        run(current + 1)
    elif command == 'next':
        respond(request)
        if current < len(lines):
            current += 1
            stop('step')
        else:
            finish()
    elif command == 'pause':
        respond(request)
        stop('pause')
    elif command == 'disconnect':
        respond(request)
        finish()
        sys.exit(0)
    else:
        respond(request)
"#;

    /// A three-line program: the breakpoint goes on `middle`.
    const PROGRAM: &str = "start\nmiddle\nend\n";

    struct Fixture {
        workbench: Entity<Workbench>,
        root: PathBuf,
        _folders: (tempfile::TempDir, tempfile::TempDir),
    }

    impl Fixture {
        fn editor(&self, cx: &mut VisualTestContext) -> Entity<Editor> {
            self.workbench.read_with(cx, |workbench, _| workbench.editor().cloned()).expect("an editor")
        }

        fn driver(&self, cx: &mut VisualTestContext) -> Entity<DebugDriver> {
            self.workbench.read_with(cx, |workbench, _| workbench.debug_driver().cloned()).expect("a debugger")
        }

        fn view(&self, cx: &mut VisualTestContext) -> Option<Entity<DebugWindowView>> {
            self.workbench.read_with(cx, |workbench, _| workbench.debug_window_view().cloned())
        }

        fn marks(&self, layer: MarkLayer, cx: &mut VisualTestContext) -> Vec<Mark> {
            self.editor(cx).read_with(cx, |editor, _| editor.marks(layer).to_vec())
        }

        /// The mark of `kind` over the 0-based `line` of the editor's document.
        fn line_mark(&self, line: u32, kind: MarkKind, cx: &mut VisualTestContext) -> Mark {
            self.editor(cx).read_with(cx, |editor, _| Mark::new(buffer::line_range(editor.rope(), line), kind))
        }

        fn caret_to(&self, line: u32, cx: &mut VisualTestContext) {
            self.editor(cx).update(cx, |editor, cx| {
                let at = editor.offset_of(line, 0);
                editor.select(at, at, cx);
            });
        }

        fn status(&self, cx: &mut VisualTestContext) -> Option<SessionStatus> {
            let driver = self.driver(cx);
            driver.read_with(cx, |driver, cx| driver.debugger().read(cx).state.active_session().map(|s| s.status))
        }

        fn console(&self, cx: &mut VisualTestContext) -> String {
            let driver = self.driver(cx);
            driver.read_with(cx, |driver, cx| {
                let state = &driver.debugger().read(cx).state;
                let lines = state.active().map(|id| state.console(id)).unwrap_or_default();
                lines.iter().map(|line| line.text.as_str()).collect()
            })
        }

        fn frame_lines(&self, cx: &mut VisualTestContext) -> Vec<String> {
            let Some(view) = self.view(cx) else { return Vec::new() };
            let frames = view.read_with(cx, |view, _| view.frames().clone());
            frames.update(cx, |frames, cx| frames.frame_lines(cx))
        }

        fn variable_lines(&self, cx: &mut VisualTestContext) -> Vec<String> {
            let Some(view) = self.view(cx) else { return Vec::new() };
            let variables = view.read_with(cx, |view, _| view.variables().clone());
            variables.read_with(cx, |variables, cx| variables.variable_lines(cx))
        }

        /// Whether the local `line` is listed with `value`.
        fn shows_line_value(&self, value: &str, cx: &mut VisualTestContext) -> bool {
            self.variable_lines(cx).iter().any(|row| row.trim_start().starts_with("line") && row.ends_with(value))
        }

        /// Selects the project's configuration once the run driver has listed it.
        fn select_main(&self, cx: &mut VisualTestContext) {
            let run = self.workbench.read_with(cx, |workbench, _| workbench.run_driver().cloned()).expect("runs");
            let store = run.read_with(cx, |run, _| run.store().clone());
            long(cx, "listing the configurations", &|cx| store.read_with(cx, |store, _| store.loaded));
            run.update(cx, |run, cx| run.select_config(Some("main"), cx));
        }
    }

    /// A trusted project with `program` in `main.fake`, debugged by the fake adapter through the
    /// configuration `main`, open in a window with `main.fake` in the editor.
    fn open<'a>(cx: &'a mut TestAppContext, program: &str) -> (Fixture, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
        let dirs = AppDirs::under(home.path());
        let script = home.path().join("fake_dap.py");
        std::fs::write(&script, FAKE_ADAPTER).expect("the adapter script is written");
        let languages = dirs.config.join("languages");
        std::fs::create_dir_all(&languages).expect("the languages folder is made");
        let toml = format!(
            "id = \"fakedap\"\nname = \"Fake\"\nextensions = [\"fake\"]\nline-comment = \"#\"\n\n\
             [debugger]\nadapter = \"fake-dap\"\ncommand = \"python3\"\nargs = [{:?}]\n",
            script.to_string_lossy(),
        );
        std::fs::write(languages.join("fakedap.toml"), toml).expect("the language is written");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("main.fake"), program).expect("the program is written");
        let config = ide_run::RunConfig {
            command: "main.fake".to_owned(),
            debug_adapter: Some("fake-dap".to_owned()),
            ..blank_config("main", "main", "program")
        };
        ide_run::save_user_configs(&root, &[config]).expect("the configuration is saved");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            crate::editor::actions::register(cx);
            super::super::register_actions(cx);
            cx.set_global(UiStorage::default());
            Backend::init(dirs, cx).expect("the backend starts");
            super::super::menu::init(cx);
            cx.global::<Backend>().host.trust.decide(&root, true, false).expect("the project is trusted");
        });
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        long(cx, "the project to open", &|cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let file = root.join("main.fake");
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file, window, cx));
        long(cx, "the editor", &|cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        (Fixture { workbench, root, _folders: (home, folder) }, cx)
    }

    /// Lets the backend, the adapter and the timers run, real time passing, until `done` holds.
    fn long(cx: &mut VisualTestContext, what: &str, done: &dyn Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..3000 {
            cx.executor().advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what} did not happen within thirty seconds");
    }

    fn run(cx: &mut VisualTestContext, id: &str) {
        let ran = cx.update(|window, cx| execute_action_or_throw(id, None, window, cx));
        ran.unwrap_or_else(|error| panic!("{id}: {error}"));
    }

    fn context(cx: &mut VisualTestContext, key: &str) -> bool {
        cx.update(|window, cx| window_context(window, cx).is_truthy(key))
    }

    fn close_project(fixture: &Fixture, cx: &mut VisualTestContext) {
        fixture.workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
        cx.run_until_parked();
    }

    // Not from a spec: Toggle Line Breakpoint at the caret marks the line in the editor, stores the
    // breakpoint for the project, lists it in View Breakpoints, and Mute Breakpoints greys it.
    #[gpui_kit::test]
    async fn toggling_a_breakpoint_at_a_line_marks_it_persists_it_and_lists_it_in_the_breakpoints_dialog(
        cx: &mut TestAppContext,
    ) {
        let (fixture, cx) = open(cx, PROGRAM);
        fixture.caret_to(1, cx);
        run(cx, "debug.toggleBreakpoint");
        let marked = fixture.line_mark(1, MarkKind::BreakpointLine, cx);
        long(cx, "the breakpoint mark", &|cx| fixture.marks(MarkLayer::Breakpoints, cx) == [marked.clone()]);
        let key = storage_key(&fixture.root.to_string_lossy());
        let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(&key).map(str::to_owned));
        assert!(stored.as_deref().is_some_and(|value| value.contains("\"line\":1")), "{stored:?}");

        run(cx, "debug.viewBreakpoints");
        let dialog_open =
            |cx: &mut VisualTestContext| fixture.workbench.read_with(cx, |w, _| w.breakpoints_dialog().is_some());
        long(cx, "the Breakpoints dialog", &dialog_open);
        let dialog = fixture.workbench.read_with(cx, |w, _| w.breakpoints_dialog().cloned()).expect("the dialog");
        let listed = dialog.read_with(cx, |dialog, _| {
            dialog.rows().iter().any(|row| matches!(row, DialogRow::Line { entry, .. } if entry.line == 1))
        });
        assert!(listed);

        run(cx, "debug.muteBreakpoints");
        let muted = fixture.line_mark(1, MarkKind::InactiveBreakpoint, cx);
        long(cx, "the muted mark", &|cx| fixture.marks(MarkLayer::Breakpoints, cx) == [muted.clone()]);
        assert!(dialog.read_with(cx, |dialog, _| dialog.muted()));
        dialog.update(cx, |_, cx| cx.emit(BreakpointsDialogEvent::Close));
        long(cx, "the dialog to close", &|cx| !dialog_open(cx));
        close_project(&fixture, cx);
    }

    // Not from a spec: a click in the gutter toggles the line's breakpoint.
    #[gpui_kit::test]
    async fn clicking_the_gutter_toggles_the_lines_breakpoint(cx: &mut TestAppContext) {
        let (fixture, cx) = open(cx, PROGRAM);
        let editor = fixture.editor(cx);
        let at = editor.read_with(cx, |editor, _| editor.position_of(editor.offset_of(2, 0)));
        let at = at.expect("line 3 is painted");
        let press = |cx: &mut VisualTestContext| {
            let position = point(at.x - gpui_kit::px(12.0), at.y + gpui_kit::px(4.0));
            cx.simulate_event(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            });
        };
        press(cx);
        let marked = fixture.line_mark(2, MarkKind::BreakpointLine, cx);
        long(cx, "the breakpoint mark", &|cx| fixture.marks(MarkLayer::Breakpoints, cx) == [marked.clone()]);
        press(cx);
        long(cx, "the mark to go", &|cx| fixture.marks(MarkLayer::Breakpoints, cx).is_empty());
        close_project(&fixture, cx);
    }

    // Not from a spec: Run › Debug starts the selected configuration under the debugger. The Debug
    // tool window shows its tab, the program stops at the breakpoint with its frame listed, the
    // execution line drawn and the local shown, Step Over moves the line, and Resume runs the
    // program to its end, which the tab shows.
    #[gpui_kit::test]
    async fn debugging_a_configuration_stops_at_the_breakpoint_steps_over_and_resumes_to_the_end(
        cx: &mut TestAppContext,
    ) {
        let (fixture, cx) = open(cx, PROGRAM);
        fixture.caret_to(1, cx);
        run(cx, "debug.toggleBreakpoint");
        fixture.select_main(cx);
        run(cx, "run.debug");
        long(cx, "the stop at the breakpoint", &|cx| context(cx, "debugSessionPaused"));
        assert!(fixture.workbench.read_with(cx, |w, _| w.is_tool_window_visible(DEBUG_TOOL_WINDOW)));
        let view = fixture.view(cx).expect("the Debug tool window");
        assert_eq!(view.update(cx, |view, cx| view.tab_titles(cx)), ["main"]);
        long(cx, "the frame", &|cx| fixture.frame_lines(cx) == ["main, main.fake:2"]);
        let execution = fixture.line_mark(1, MarkKind::ExecutionLine, cx);
        long(cx, "the execution line", &|cx| fixture.marks(MarkLayer::Execution, cx) == [execution.clone()]);
        long(cx, "the local", &|cx| fixture.shows_line_value("2", cx));

        run(cx, "debug.stepOver");
        let stepped = fixture.line_mark(2, MarkKind::ExecutionLine, cx);
        long(cx, "the step", &|cx| fixture.marks(MarkLayer::Execution, cx) == [stepped.clone()]);
        long(cx, "the stepped frame", &|cx| fixture.frame_lines(cx) == ["main, main.fake:3"]);
        long(cx, "the local after the step", &|cx| fixture.shows_line_value("3", cx));

        run(cx, "debug.resume");
        long(cx, "the end of the program", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
        assert_eq!(view.read_with(cx, |view, cx| view.finished_tabs(cx)), [true]);
        assert!(fixture.console(cx).contains("Process finished with exit code 0"), "{}", fixture.console(cx));
        assert!(fixture.marks(MarkLayer::Execution, cx).is_empty());
        assert!(!context(cx, "debugSessionActive"));
        close_project(&fixture, cx);
    }

    // Not from a spec: Stop ends a session whose program is running.
    #[gpui_kit::test]
    async fn stop_ends_a_running_debug_session(cx: &mut TestAppContext) {
        let (fixture, cx) = open(cx, "start\nloop\nend\n");
        fixture.select_main(cx);
        run(cx, "run.debug");
        long(cx, "the running session", &|cx| context(cx, "debugSessionRunning"));
        run(cx, "debug.stop");
        long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
        let view = fixture.view(cx).expect("the Debug tool window");
        assert_eq!(view.read_with(cx, |view, cx| view.finished_tabs(cx)), [true]);
        assert!(!context(cx, "debugSessionActive"));
        close_project(&fixture, cx);
    }
}
```

The unit tests in `driver.rs` need no adapter. They cover:

- Toggled breakpoints are stored, sent once the edits rest, read back by a new driver, and removed again.
- Mute is stored and read back.
- A start whose adapter is missing: `bash-debug`, with a user `shellscript.toml` whose command does not exist. It notifies "Bash Debug is not installed" with "Install and Debug", and that button asks `tools.install` for `bash-debug`.

## 6. Assumptions I could not confirm by reading

- **`driver.rs` `with_host`:**
  - `cx.update_global::<Localization, _>(|localization, cx| …)` on a `Context<DebugDriver>` hands the closure `&mut Context<DebugDriver>`. It is `BorrowAppContext::update_global`, as `workbench/project_files.rs` uses it.
  - `debugger.update(cx, …)` inside it compiles.
  - Check: `gpui-base-0.9.1` / `gpui_kit` `BorrowAppContext`.
- **`driver.rs` closure types:** closures typed `FnOnce(&mut Debugger, Result<T, IdeError>, &mut Host)` (a struct with a hidden lifetime, in `Fn` sugar) infer their argument types when passed straight to `Self::on_debugger`. If inference fails, annotate the closure arguments.
- **`driver.rs` `copy_value`:**
  - `tokio::runtime::Handle::current()` works inside `host.blocking` (`spawn_blocking` on the backend's runtime).
  - `runtime.block_on` of `debug_variables` from there does not deadlock.
  - Check: `ide_host::ops::Host::blocking` in `crates/ide-host/src/ops/mod.rs`.
- **`driver.rs` backend return types:**
  - `ide_host::ops::java::java_resolve_launch` returns `ide_lsp::jdtls::JavaLaunchInfo`.
  - `ops::fs::read_dir` returns `ide_vfs::FileEntry`.
  - Both must be the types `Debugger::launch_resolved` / `dir_read` take.
- **`driver.rs` other backend calls:**
  - `ide_host::ops::run::run_dotnet_resolve` is reachable through `pub use commands::*` in `ops/run.rs`.
  - `host.languages.all()` items have `id: LanguageId` (`as_str`) and `debugger: Option<_>`.
- **`driver.rs` `Env.run` type:** `ide_core::settings::Settings::run` is `ide_core::workflow_settings::RunSettings`, which is what `Env.run` takes. Check the `use` at the top of `crates/ide-core/src/settings.rs`.
- **`driver.rs` `evaluate`:** `BackendCall::DebugEvaluateIn`'s `context` is `EvaluateContext` (`as_str`). Confirmed in `effects.rs`. Its `frame` is `Option<i64>`.
- **`debug.rs` `publish_debug_context`:** `(DebugContext, Option<DebugViewCommands>)::default()` needs both to be `Default`. `DebugContext` derives it; `Option` is fine.
- **`debug.rs` `confirm_close_session`:**
  - `window.prompt(level, &str, Option<&str>, &[&str], cx)` returns a `futures::channel::oneshot::Receiver<usize>`.
  - `Canceled: PartialEq`, for `answer.await != Ok(0)`.
  - `RunDriver::ask` uses the same `prompt` call.
- **`debug.rs` `observe_in` / `subscribe_in`:** these closure signatures are copied from `vcs/commit_box.rs` and `workbench/database.rs`.
- **`debug.rs` `open_debug_file`:** `open_project_file_at` focuses the editor. For `focus: false` (a stop's reveal), the keyboard is given back to what had it (`window.focused` before, `window.focus` after). If that leaves the Debug tool window without the keyboard, drop the restore.
- **`debug.rs` `run_debug_action`:** `debugger.overlays.chooser.take()` sends Debug… with several debuggable configurations to the native chooser (`choose_config(ChooserMode::Debug)`). Check that `ChooserMode::Debug` makes a chosen row emit `ChooserAction::Debug` (`run/chooser_view.rs`).
- **`view.rs` §4.6:** `Hsla::opacity(0.0)` gives a transparent fill, and `quad(bounds, radius, fill, border_width, border_color, style)` takes `Pixels` for the radius and the border width, as in the `CurrentFindOccurrence` paint.
- **E2E test `clicking_the_gutter_…`:**
  - 12 px left of the text start is inside the gutter (`w-gutter` wider than 12 px).
  - `MouseDownEvent` has exactly the fields `button, position, modifiers, click_count, first_mouse`. Check the press in `editor/view.rs` tests (≈ line 2538).
- **E2E tests, test drawing:** context keys are republished on every draw, and `run_until_parked` draws a dirty test window. The Run e2e test in `workbench/mod.rs` relies on the same thing for `run.run`'s `projectOpen`.
- **E2E tests, variable row text:** the Variables view lists the expanded `Locals` scope's child as `line = {int} 2`, possibly indented. The test only checks that a row starts with `line` and ends with the value.
- **E2E tests, missing-adapter error:** the backend's error for a missing command is `debug adapter \`bash-debug\` not found …`with`ErrorKind::Unavailable` (`ide_dap::adapter::missing`, reached at `adapter.rs`≈ 470).`ide_sdk::apply::tool_command`finds no tool named`openidea-test-missing-bash-debug`.
- **Language files:** a user language file needs no fields beyond `id`, `name`, `extensions`, `line-comment` and `[debugger] adapter / command / args`. Check `ide_language::LanguageConfig` serde defaults.

## 7. Not wired

- **Evaluate Expression, Attach to Process, Smart Step Into's in-editor picking, Quick Evaluate's value hint, the breakpoint properties popover, the debug console input and history:** none has a view. Their actions (`debug.evaluate`, `debug.attachToProcess`, `debug.smartStepInto`, `debug.quickEvaluate`, `debug.browseConsoleHistory`, `debug.attach`, `debug.pluginStart`) are not registered. Smart Step Into's `DebugStepInTargets` answer is handed to the model with no paused line (`None`), which is the chooser path, and the chooser isn't drawn.
- **The parts of `run.debugConfig` beyond `debug.startConfig`:** `run.debugConfig` resolves its configuration (`RunDriver::config_from_arg`) and hands it to `debug.startConfig`. Not built from `src/features/run/index.ts::debugConfig`:
  - compound configurations;
  - before-launch tasks (`prepareLaunch`);
  - the remote-debug block check;
  - `recordRun`;
  - plugin run targets;
  - Python in a container (`debugInContainer`);
  - single compiled files (`compiledDebugPlan`);
  - Gradle delegation and the Maven / Gradle JDWP agent (`debugWithAgent`).

  `run.debug` with "Current File" selected runs `run.debugContext`, which has no native handler yet.

- **Plugin debug runs:** `PluginPrepareDebugRun` is answered with an error. `ide_host::Host::plugin_host` is `pub(crate)`, so plugin preparations are unreachable from this crate. `take_plugin_events` is drained and logged.
- **The taskbar flash:** there is no shell API. `WindowFlashTaskbar(true)` is answered with `taskbar_flash_failed`. `DebugDriver::set_window_focused` exists but no window activation listener calls it, so `Env.window_focused` stays `true`.
- **"Install and Debug" afterwards:** `tools.install` is run as an action. Nothing calls `Debugger::adapter_installed` once the install ends, so the launch is not repeated.
- **Breakpoints following edits:** `breakpoint_gutter::document_changed` is not called from `EditorEvent::Edited`, so breakpoints don't move when lines are inserted above them. The marks do (mark mapping), until the next debugger change redraws them from the store.
- **Gutter:**
  - The middle click (enable / disable), the right click (properties popover) and dragging across the gutter are not wired.
  - Only the left press reaches the gutter, and it counts as a click.
  - Shift-click and Alt-click work: logging breakpoint (opens the Breakpoints dialog on it), and temporary breakpoint.
- **Run to Cursor's inlay, inline values after line ends, value tooltips:** none is drawn. The execution point's locals are loaded, but nothing shows them.
- **Jump to Source of a local** goes to the selected frame's line, not to the local's declaration.
- **Settings changes reaching the Variables view:** `set_sort_values` is read when the tool window mounts, not on `SettingsChanged`.
- **Status bar messages** (`Effect::StatusInfo`) are shown as Info notifications with key `debug.status`, since the workbench has no transient status message.
- **Run and Services:**
  - `run_configs.rs::RunContext::of` still passes no debug sessions to `updatable_sessions`.
  - The Services window's `ServiceDetail::DebugConsole` still shows nothing.
  - Debug sessions in Stop's menu (`stoppable_sessions`) are not offered beside run processes.
- **`debug.cancelHotSwap`'s `debugHotSwapBuilding` key** is not published, so Cancel Hot Swap Build stays disabled.
