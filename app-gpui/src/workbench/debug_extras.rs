//! The debugger actions that need a view of their own: Evaluate Expression (`debug.evaluate`) opens
//! the Evaluate dialog over the window, and Smart Step Into (`debug.smartStepInto`) steps into a call
//! of the paused line, the call chosen in a menu when the debugger lists several.
//!
//! The debugger's model decides what each action does (the dialog's expression, whether to step
//! straight in or ask); this module shows what it asks for and hands the answers back.

use super::Workbench;
use super::services_window::on_workbench;
use crate::debugger::effects::EvaluateContext;
use crate::debugger::evaluate_dialog::{
    EvaluateDialog, EvaluateDialogEvent, history_key, pushed_history, read_history,
};
use crate::platform::actions::ActionRegistry;
use crate::platform::storage::UiStorage;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, Subscription, Window, div, point,
    prelude::*, px,
};
use ide_core::DebugSessionId;
use serde_json::Value;

/// Evaluate Expression…
pub(super) const EVALUATE: &str = "debug.evaluate";
/// Smart Step Into.
pub(super) const SMART_STEP_INTO: &str = "debug.smartStepInto";
/// The storage kind of the Evaluate dialog's history.
const EVALUATE_HISTORY: &str = "evaluate";

/// What the window keeps for these actions.
#[derive(Default)]
pub(super) struct DebugExtrasSlot {
    /// The Evaluate dialog, while it is open.
    evaluate: Option<EvaluateWindow>,
}

/// The Evaluate dialog over the window.
struct EvaluateWindow {
    dialog: Entity<EvaluateDialog>,
    /// What had the keyboard before the dialog opened.
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// A call of the paused line chosen in Smart Step Into's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartStepChoice {
    pub session: DebugSessionId,
    pub thread: i64,
    pub target: i64,
}

/// Registers Evaluate Expression and Smart Step Into.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle(EVALUATE, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.evaluate_expression(arg.as_ref(), window, cx))
    });
    registry.handle(SMART_STEP_INTO, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.run_debug_action(SMART_STEP_INTO, arg.as_ref(), window, cx);
        })
    });
}

impl Workbench {
    /// The Evaluate dialog, while it is open.
    pub fn evaluate_dialog(&self) -> Option<&Entity<EvaluateDialog>> {
        self.debug_extras.evaluate.as_ref().map(|open| &open.dialog)
    }

    // ---------------------------------------------------------------------------------------------
    // Evaluate Expression

    /// Evaluate Expression…: the debugger seeds the dialog with the selection (or the expression at
    /// the caret) of the active editor, and the dialog opens on it.
    pub fn evaluate_expression(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        self.run_debug_action(EVALUATE, arg, window, cx);
        self.sync_evaluate_dialog(window, cx);
    }

    /// The storage key of the project's Evaluate history.
    fn evaluate_history_key(&self, cx: &App) -> Option<String> {
        let root = self.project_root(cx)?;
        Some(history_key(EVALUATE_HISTORY, &root.to_string_lossy()))
    }

    /// Opens the dialog the debugger asks for; an open one only takes the keyboard again.
    fn sync_evaluate_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(driver) = self.debug_driver().cloned() else { return };
        let debugger = driver.read(cx).debugger().clone();
        let Some(expression) = debugger.read(cx).overlays.evaluate.clone() else { return };
        if let Some(open) = &self.debug_extras.evaluate {
            let focus = open.dialog.focus_handle(cx);
            window.focus(&focus, cx);
            return;
        }
        let key = self.evaluate_history_key(cx);
        let history = key.map(|key| read_history(cx.default_global::<UiStorage>(), &key)).unwrap_or_default();
        let dialog = cx.new(|cx| EvaluateDialog::new(debugger, &expression, history, window, cx));
        let events = cx.subscribe_in(&dialog, window, Self::evaluate_dialog_event);
        let restore = window.focused(cx);
        window.focus(&dialog.focus_handle(cx), cx);
        self.debug_extras.evaluate = Some(EvaluateWindow { dialog, restore, _events: events });
        cx.notify();
    }

    fn evaluate_dialog_event(
        &mut self,
        dialog: &Entity<EvaluateDialog>,
        event: &EvaluateDialogEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.debug_driver().cloned() else { return };
        match event.clone() {
            EvaluateDialogEvent::Evaluate { request, expression } => {
                if let Some(key) = self.evaluate_history_key(cx) {
                    let storage = cx.default_global::<UiStorage>();
                    let history = pushed_history(&read_history(storage, &key), &expression);
                    match serde_json::to_string(&history) {
                        Ok(value) => storage.set(&key, value),
                        Err(error) => tracing::warn!("cannot store the Evaluate history: {error}"),
                    }
                }
                let session = driver.read(cx).debugger().read(cx).state.active();
                let Some(session) = session else {
                    let answer = Err("No active debug session".to_owned());
                    dialog.update(cx, |dialog, cx| dialog.evaluated(request, answer, cx));
                    return;
                };
                let evaluating = driver.read(cx).evaluate(session, &expression, EvaluateContext::Repl, cx);
                let dialog = dialog.downgrade();
                cx.spawn(async move |_, cx| {
                    let answer = evaluating.await;
                    // The dialog was closed meanwhile: nobody waits for the value.
                    let _ = dialog.update(cx, |dialog, cx| dialog.evaluated(request, answer, cx));
                })
                .detach();
            }
            EvaluateDialogEvent::AddWatch { expression, close } => {
                driver.update(cx, |driver, cx| driver.change(cx, |debugger, _| debugger.state.add_watch(&expression)));
                if close {
                    self.close_evaluate_dialog(window, cx);
                }
            }
            EvaluateDialogEvent::Closed => self.close_evaluate_dialog(window, cx),
        }
    }

    /// Closes the dialog: the debugger forgets it, and the keyboard goes back to what had it before.
    fn close_evaluate_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.debug_extras.evaluate.take() else { return };
        if let Some(driver) = self.debug_driver().cloned() {
            driver.update(cx, |driver, cx| driver.change(cx, |debugger, _| debugger.overlays.evaluate = None));
        }
        if let Some(target) = open.restore.or_else(|| self.editor().map(|editor| editor.focus_handle(cx))) {
            window.focus(&target, cx);
        }
        cx.notify();
    }

    /// The Evaluate dialog over the window, while it is open.
    pub(super) fn debug_evaluate_layer(&self, cx: &App) -> Option<AnyElement> {
        let dialog = self.evaluate_dialog()?.clone();
        let layer = div()
            .id("debug-evaluate-layer")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(super::dialog_frame(dialog, cx));
        Some(gpui_kit::deferred(layer).with_priority(1).into_any_element())
    }

    // ---------------------------------------------------------------------------------------------
    // Smart Step Into

    /// The debugger listed several calls on the paused line: they are offered in a menu near the top
    /// of the window, and the one chosen is stepped into.
    pub(super) fn sync_smart_step_chooser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(driver) = self.debug_driver().cloned() else { return };
        let debugger = driver.read(cx).debugger().clone();
        let Some(step) = debugger.update(cx, |debugger, _| debugger.overlays.smart_step.take()) else { return };
        let items: Vec<MenuItem<SmartStepChoice>> = step
            .targets
            .iter()
            .map(|target| {
                let choice = SmartStepChoice { session: step.session, thread: step.thread, target: target.id };
                let mut item = MenuItem::item(target.label.clone(), choice);
                if let MenuItem::Item(entry) = &mut item {
                    entry.icon = Some("method");
                }
                item
            })
            .collect();
        let width = f32::from(window.viewport_size().width);
        let position = point(px((width / 2.0 - 210.0).max(16.0)), px(80.0));
        let step_into = |workbench: &mut Self, choice: SmartStepChoice, _: &mut Window, cx: &mut Context<Self>| {
            workbench.step_into_target(choice, cx);
        };
        self.show_menu(&items, position, step_into, window, cx);
    }

    /// Steps the paused thread into the chosen call.
    fn step_into_target(&mut self, choice: SmartStepChoice, cx: &mut Context<Self>) {
        let Some(driver) = self.debug_driver().cloned() else { return };
        driver.update(cx, |driver, cx| {
            driver.change(cx, |debugger, host| {
                debugger.step_into_target(choice.session, choice.thread, choice.target, host);
            });
        });
    }
}

#[cfg(test)]
pub(in crate::workbench) mod tests {
    use super::*;
    use crate::backend::{AppDirs, Backend};
    use crate::debugger::driver::DebugDriver;
    use crate::debugger::evaluate_dialog::ADD_TO_WATCHES;
    use crate::debugger::state::SessionStatus;
    use crate::editor::view::Editor;
    use crate::platform::actions::execute_action_or_throw;
    use crate::platform::context::window_context;
    use crate::platform::input;
    use crate::run::configs::blank_config;
    use crate::run::store::RunStore;
    use crate::theme;
    use crate::ui::popup_menu::{Choice, PopupMenuEvent};
    use crate::workbench::debug::tests::FAKE_ADAPTER;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::path::PathBuf;
    use std::time::Duration;

    /// A three-line program: the breakpoint goes on `middle`.
    pub(in crate::workbench) const PROGRAM: &str = "start\nmiddle\nend\n";

    /// A trusted project in a window, with its folders.
    pub(in crate::workbench) struct Fixture {
        pub workbench: Entity<Workbench>,
        pub root: PathBuf,
        _folders: (tempfile::TempDir, tempfile::TempDir),
    }

    impl Fixture {
        pub fn editor(&self, cx: &mut VisualTestContext) -> Entity<Editor> {
            self.workbench.read_with(cx, |workbench, _| workbench.editor().cloned()).expect("an editor")
        }

        pub fn driver(&self, cx: &mut VisualTestContext) -> Entity<DebugDriver> {
            self.workbench.read_with(cx, |workbench, _| workbench.debug_driver().cloned()).expect("a debugger")
        }

        pub fn run_store(&self, cx: &mut VisualTestContext) -> Entity<RunStore> {
            let run = self.workbench.read_with(cx, |workbench, _| workbench.run_driver().cloned()).expect("runs");
            run.read_with(cx, |run, _| run.store().clone())
        }

        pub fn caret_to(&self, line: u32, cx: &mut VisualTestContext) {
            self.editor(cx).update(cx, |editor, cx| {
                let at = editor.offset_of(line, 0);
                editor.select(at, at, cx);
            });
        }

        pub fn status(&self, cx: &mut VisualTestContext) -> Option<SessionStatus> {
            let driver = self.driver(cx);
            driver.read_with(cx, |driver, cx| driver.debugger().read(cx).state.active_session().map(|s| s.status))
        }

        /// The Frames pane's rows, once the Debug tool window was shown.
        pub fn frame_lines(&self, cx: &mut VisualTestContext) -> Vec<String> {
            let Some(view) = self.workbench.read_with(cx, |workbench, _| workbench.debug_window_view().cloned()) else {
                return Vec::new();
            };
            let frames = view.read_with(cx, |view, _| view.frames().clone());
            frames.update(cx, |frames, cx| frames.frame_lines(cx))
        }

        /// The Variables pane's rows, once the Debug tool window was shown.
        pub fn variable_lines(&self, cx: &mut VisualTestContext) -> Vec<String> {
            let Some(view) = self.workbench.read_with(cx, |workbench, _| workbench.debug_window_view().cloned()) else {
                return Vec::new();
            };
            let variables = view.read_with(cx, |view, _| view.variables().clone());
            variables.read_with(cx, |variables, cx| variables.variable_lines(cx))
        }

        /// Selects the project's configuration `main` once the run driver has listed it.
        pub fn select_main(&self, cx: &mut VisualTestContext) {
            let run = self.workbench.read_with(cx, |workbench, _| workbench.run_driver().cloned()).expect("runs");
            let store = run.read_with(cx, |run, _| run.store().clone());
            long(cx, "listing the configurations", &|cx| store.read_with(cx, |store, _| store.loaded));
            run.update(cx, |run, cx| run.select_config(Some("main"), cx));
        }

        pub fn close_project(&self, cx: &mut VisualTestContext) {
            self.workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
            cx.run_until_parked();
        }
    }

    /// A trusted project with `files`, whose configuration `main` debugs `main.fake` with the fake
    /// debug adapter `adapter` (a Python script), open in a window; `open` is opened in the editor.
    pub(in crate::workbench) fn open_project<'a>(
        cx: &'a mut TestAppContext,
        adapter: &str,
        files: &[(&str, &str)],
        open: Option<&str>,
    ) -> (Fixture, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
        let dirs = AppDirs::under(home.path());
        let script = home.path().join("fake_dap.py");
        std::fs::write(&script, adapter).expect("the adapter script is written");
        let languages = dirs.config.join("languages");
        std::fs::create_dir_all(&languages).expect("the languages folder is made");
        let toml = format!(
            "id = \"fakedap\"\nname = \"Fake\"\nextensions = [\"fake\"]\nline-comment = \"#\"\n\n\
             [debugger]\nadapter = \"fake-dap\"\ncommand = \"python3\"\nargs = [{:?}]\n",
            script.to_string_lossy(),
        );
        std::fs::write(languages.join("fakedap.toml"), toml).expect("the language is written");
        let root = folder.path().canonicalize().expect("the folder exists");
        for (name, text) in files {
            std::fs::write(root.join(name), text).expect("the file is written");
        }
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
        if let Some(name) = open {
            let file = root.join(name);
            workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file, window, cx));
            long(cx, "the editor", &|cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        }
        (Fixture { workbench, root, _folders: (home, folder) }, cx)
    }

    /// Lets the backend, the processes and the timers run, real time passing, until `done` holds.
    pub(in crate::workbench) fn long(
        cx: &mut VisualTestContext,
        what: &str,
        done: &dyn Fn(&mut VisualTestContext) -> bool,
    ) {
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

    /// Runs the action `id` through the registry, as a shortcut or a menu does.
    pub(in crate::workbench) fn run(cx: &mut VisualTestContext, id: &str, arg: Option<Value>) {
        let ran = cx.update(|window, cx| execute_action_or_throw(id, arg.as_ref(), window, cx));
        ran.unwrap_or_else(|error| panic!("{id}: {error}"));
    }

    pub(in crate::workbench) fn context(cx: &mut VisualTestContext, key: &str) -> bool {
        cx.update(|window, cx| window_context(window, cx).is_truthy(key))
    }

    /// Breaks on the program's second line: the project's `main` runs under the debugger and stops
    /// there.
    fn stop_on_second_line(fixture: &Fixture, cx: &mut VisualTestContext) {
        fixture.caret_to(1, cx);
        run(cx, "debug.toggleBreakpoint", None);
        fixture.select_main(cx);
        run(cx, "run.debug", None);
        long(cx, "the stop at the breakpoint", &|cx| context(cx, "debugSessionPaused"));
        long(cx, "the frame", &|cx| fixture.frame_lines(cx) == ["main, main.fake:2"]);
    }

    /// The fake adapter, also listing two calls on every line: `parse()` (id 1) and `total()` (id 2).
    /// Stepping into `total()` moves two lines down, any other step one.
    fn adapter_with_calls() -> String {
        let calls = concat!(
            "    elif command == 'stepInTargets':\n",
            "        respond(request, {'targets': [{'id': 1, 'label': 'parse()'},\n",
            "                                      {'id': 2, 'label': 'total()'}]})\n",
            "    elif command == 'stepIn' and arguments.get('targetId') == 2:\n",
            "        respond(request)\n",
            "        current += 2\n",
            "        stop('step')\n",
            "    elif command == 'continue':",
        );
        let capabilities = "'supportsSetVariable': True}";
        let with_targets = "'supportsSetVariable': True, 'supportsStepInTargetsRequest': True}";
        let adapter =
            FAKE_ADAPTER.replace(capabilities, with_targets).replace("    elif command == 'continue':", calls);
        assert!(adapter.contains("supportsStepInTargetsRequest") && adapter.contains("'stepInTargets'"));
        adapter
    }

    // Not from a spec: Evaluate Expression… opens the Evaluate dialog on the editor's selection; Enter
    // evaluates it in the paused frame and shows its value, the expression goes into the project's
    // history, and Add to Watches adds it to the watches of the Variables pane and closes the dialog.
    #[gpui_kit::test]
    async fn evaluate_expression_evaluates_the_selection_in_the_paused_frame_and_adds_it_to_the_watches(
        cx: &mut TestAppContext,
    ) {
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("main.fake", PROGRAM)], Some("main.fake"));
        assert!(!context(cx, "debugSessionActive"), "Evaluate Expression… needs a session");
        stop_on_second_line(&fixture, cx);
        fixture.editor(cx).update(cx, |editor, cx| {
            let from = editor.offset_of(1, 0);
            editor.select(from, from + 6, cx);
        });
        run(cx, EVALUATE, None);
        let dialog = fixture.workbench.read_with(cx, |w, _| w.evaluate_dialog().cloned()).expect("the Evaluate dialog");
        assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.expression(cx)), "middle");
        assert!(cx.update(|window, cx| dialog.focus_handle(cx).is_focused(window)), "the field has the keyboard");

        cx.simulate_keystrokes("enter");
        let shown = |cx: &mut VisualTestContext| dialog.read_with(cx, |dialog, _| dialog.result_line());
        long(cx, "the value", &|cx| shown(cx).as_deref() == Some("middle = value of middle"));
        let key = history_key(EVALUATE_HISTORY, &fixture.root.to_string_lossy());
        let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(&key).map(str::to_owned));
        assert_eq!(stored.as_deref(), Some("[\"middle\"]"));

        dialog.update(cx, |dialog, cx| dialog.press(ADD_TO_WATCHES, cx));
        cx.run_until_parked();
        assert!(fixture.workbench.read_with(cx, |w, _| w.evaluate_dialog().is_none()), "Add to Watches closes it");
        let driver = fixture.driver(cx);
        let watches = driver.read_with(cx, |driver, cx| driver.debugger().read(cx).state.watches().to_vec());
        assert_eq!(watches, ["middle"]);
        let overlay = driver.read_with(cx, |driver, cx| driver.debugger().read(cx).overlays.evaluate.clone());
        assert_eq!(overlay, None, "the debugger forgets the dialog");
        let watched = |cx: &mut VisualTestContext| {
            fixture.variable_lines(cx).iter().any(|row| row.trim_start().starts_with("middle"))
        };
        long(cx, "the watch row", &watched);
        run(cx, "debug.stop", None);
        long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
        fixture.close_project(cx);
    }

    // Not from a spec: a debugger that cannot list the calls of the paused line steps straight into
    // it; no menu opens.
    #[gpui_kit::test]
    async fn smart_step_into_steps_straight_in_when_the_debugger_lists_no_calls(cx: &mut TestAppContext) {
        let program = "start\nmiddle\nend\nlast\n";
        let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("main.fake", program)], Some("main.fake"));
        stop_on_second_line(&fixture, cx);
        run(cx, SMART_STEP_INTO, None);
        long(cx, "the step", &|cx| fixture.frame_lines(cx) == ["main, main.fake:3"]);
        assert!(fixture.workbench.read_with(cx, |w, _| w.open_menu::<SmartStepChoice>().is_none()));
        run(cx, "debug.stop", None);
        long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
        fixture.close_project(cx);
    }

    // Not from a spec: with several calls on the paused line Smart Step Into lists them in a menu;
    // the call chosen is stepped into.
    #[gpui_kit::test]
    async fn smart_step_into_lists_the_calls_of_the_line_and_steps_into_the_chosen_one(cx: &mut TestAppContext) {
        let program = "start\nmiddle\nend\nlast\nfinal\n";
        let (fixture, cx) = open_project(cx, &adapter_with_calls(), &[("main.fake", program)], Some("main.fake"));
        stop_on_second_line(&fixture, cx);
        run(cx, SMART_STEP_INTO, None);
        let open_menu =
            |cx: &mut VisualTestContext| fixture.workbench.read_with(cx, |w, _| w.open_menu::<SmartStepChoice>());
        long(cx, "the calls", &|cx| open_menu(cx).is_some());
        let menu = open_menu(cx).expect("the menu of calls");
        let rows: Vec<(String, Option<Choice<SmartStepChoice>>)> = menu
            .read_with(cx, |menu, _| menu.rows().iter().map(|row| (row.label.clone(), row.choice.clone())).collect());
        let labels: Vec<&str> = rows.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(labels, ["parse()", "total()"]);
        let driver = fixture.driver(cx);
        let overlay = driver.read_with(cx, |driver, cx| driver.debugger().read(cx).overlays.smart_step.is_some());
        assert!(!overlay, "the menu holds the choice; the debugger keeps no overlay open");

        let total = rows[1].1.clone().expect("total() can be chosen");
        menu.update(cx, |_, cx| cx.emit(PopupMenuEvent::Chosen(total)));
        long(cx, "the step into total()", &|cx| fixture.frame_lines(cx) == ["main, main.fake:4"]);
        assert!(open_menu(cx).is_none(), "the menu closes");
        run(cx, "debug.stop", None);
        long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
        fixture.close_project(cx);
    }
}
