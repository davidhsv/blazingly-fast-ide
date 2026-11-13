//! The Terminal's actions that open a tab somewhere or for something, split the tool window into
//! tab groups, and close a session: Open in Terminal, New Predefined Session, Run Command in
//! Terminal, Split Right / Down (and Move), Unsplit, Close Terminal Session.
//!
//! [`TerminalPanel`] holds the tabs; every change goes through [`TerminalPanel::change`], which
//! starts the shells of the tabs that show and stores the project's tabs.

use super::services_window::on_workbench;
use super::{TERMINAL_TOOL_WINDOW, Workbench};
use crate::backend::Backend;
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::Localization;
use crate::platform::settings;
use crate::terminal::panel::TerminalPanel;
use crate::terminal::store::{
    SessionCommand, SessionStatus, SplitOrientation, Terminals, start_directory, terminate_confirmation,
};
use crate::vcs::prompt_dialogs::PromptDialog;
use crate::vcs_ops::dialogs::ConfirmRequest;
use gpui_kit::{Context, Entity, Window};
use ide_core::workflow_settings::TerminalSettings;
use serde_json::Value;

/// The string argument of an action.
fn text_arg(arg: Option<&Value>) -> Option<String> {
    arg.and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_owned)
}

/// The name of a shell's tab: its program's file name, without `.exe`.
fn shell_name(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let name = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(name);
    if name.is_empty() { path.to_owned() } else { name.to_owned() }
}

/// What Run Command in Terminal runs: `{ program, args, name?, cwd? }`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RunCommand {
    program: String,
    args: Vec<String>,
    name: Option<String>,
    cwd: Option<String>,
}

impl RunCommand {
    /// The command an argument describes; none without a program.
    fn from_arg(arg: Option<&Value>) -> Option<Self> {
        let arg = arg?;
        let text = |key: &str| arg.get(key).and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_owned);
        let program = text("program")?;
        let args = arg.get("args").and_then(Value::as_array).into_iter().flatten();
        let args = args.filter_map(Value::as_str).map(str::to_owned).collect();
        Some(Self { program, args, name: text("name"), cwd: text("cwd") })
    }
}

/// Registers the Terminal actions this module carries out.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("terminal.openAt", |call, window, cx| {
        let dir = text_arg(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.open_terminal_tab(window, cx, move |terminals, root, settings| {
                match dir {
                    Some(dir) => terminals.new_tab_at(&dir),
                    None => {
                        terminals.new_tab(&start_directory(&settings.start_directory, root), &settings.tab_name, None)
                    }
                };
            });
        })
    });
    registry.handle("terminal.newPredefinedSession", |call, window, cx| {
        let Some(shell) = text_arg(call.arg) else { return Ok(()) };
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.open_terminal_tab(window, cx, move |terminals, root, settings| {
                let cwd = start_directory(&settings.start_directory, root);
                terminals.new_tab(&cwd, &shell_name(&shell), Some(SessionCommand::predefined_shell(&shell)));
            });
        })
    });
    registry.handle("terminal.runCommand", |call, window, cx| {
        let Some(run) = RunCommand::from_arg(call.arg) else { return Ok(()) };
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.open_terminal_tab(window, cx, move |terminals, root, settings| {
                let cwd = run.cwd.unwrap_or_else(|| root.to_owned());
                let name = run.name.unwrap_or_else(|| settings.tab_name.clone());
                let command = SessionCommand { program: run.program, args: run.args, ..SessionCommand::default() };
                terminals.new_tab(&cwd, &name, Some(command));
            });
        })
    });
    for (id, orientation, move_tab) in [
        ("terminal.splitRight", SplitOrientation::Horizontal, false),
        ("terminal.splitDown", SplitOrientation::Vertical, false),
        ("terminal.splitAndMoveRight", SplitOrientation::Horizontal, true),
        ("terminal.splitAndMoveDown", SplitOrientation::Vertical, true),
    ] {
        registry.handle(id, move |call, window, cx| {
            let tab = text_arg(call.arg);
            on_workbench(window, cx, move |workbench, window, cx| {
                workbench.split_terminal(tab, orientation, move_tab, window, cx);
            })
        });
    }
    registry.handle("terminal.unsplit", |call, window, cx| {
        let group = text_arg(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| workbench.unsplit_terminal(group, window, cx))
    });
    registry.handle("terminal.closeSession", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.close_terminal_session(window, cx))
    });
}

impl Workbench {
    /// The Terminal tool window's tabs, once it has been shown.
    pub(super) fn terminal_panel(&self) -> Option<Entity<TerminalPanel>> {
        self.tool_window_view(TERMINAL_TOOL_WINDOW).and_then(|view| view.downcast::<TerminalPanel>().ok())
    }

    /// Shows the Terminal with the keyboard in it and opens the tab `open` adds (given the project
    /// root and Settings › Terminal). A Terminal shown for the first time starts with a tab of its
    /// own, which the new tab takes the place of.
    fn open_terminal_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        open: impl FnOnce(&mut Terminals, &str, &TerminalSettings),
    ) {
        let existed = self.terminal_panel().is_some();
        self.with_layout(window, cx, |layout| layout.show(TERMINAL_TOOL_WINDOW, true));
        let Some(panel) = self.terminal_panel() else { return };
        let terminal = settings::effective(&self.label, cx).terminal;
        panel.update(cx, |panel, cx| {
            let fresh = panel.take_fresh() && !existed;
            let placeholder = panel.terminals().active_tab().filter(|_| fresh).map(str::to_owned);
            let root = panel.root().to_owned();
            panel.change(cx, |terminals| open(terminals, &root, &terminal));
            if let Some(placeholder) = placeholder {
                panel.change(cx, |terminals| terminals.close_tab(&placeholder));
            }
        });
        self.with_layout(window, cx, |layout| layout.focus(TERMINAL_TOOL_WINDOW));
    }

    /// Split Right / Split Down of tab `tab` (else of the active tab) into a new group beside it,
    /// with a new shell in its directory, or with `move_tab` the tab itself. Without any tab the
    /// split is a new tab; Split and Move then does nothing.
    fn split_terminal(
        &mut self,
        tab: Option<String>,
        orientation: SplitOrientation,
        move_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = self.terminal_panel();
        let target =
            tab.or_else(|| panel.as_ref().and_then(|panel| panel.read(cx).terminals().active_tab().map(str::to_owned)));
        let Some(target) = target else {
            if !move_tab {
                self.open_terminal_tab(window, cx, |terminals, root, settings| {
                    terminals.new_tab(&start_directory(&settings.start_directory, root), &settings.tab_name, None);
                });
            }
            return;
        };
        self.with_layout(window, cx, |layout| layout.show(TERMINAL_TOOL_WINDOW, true));
        let Some(panel) = self.terminal_panel() else { return };
        let tab_name = settings::effective(&self.label, cx).terminal.tab_name;
        panel.update(cx, |panel, cx| {
            panel.change(cx, |terminals| terminals.split_tab(&target, orientation, move_tab, &tab_name));
        });
        self.with_layout(window, cx, |layout| layout.focus(TERMINAL_TOOL_WINDOW));
    }

    /// Unsplit: the tabs of `group` (else of the active tab's group) join the group beside it.
    fn unsplit_terminal(&mut self, group: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.terminal_panel() else { return };
        let group = group.or_else(|| {
            let terminals = panel.read(cx).terminals();
            terminals.active_tab().and_then(|id| terminals.tab(id)).map(|tab| tab.group.clone())
        });
        let Some(group) = group else { return };
        panel.update(cx, |panel, cx| panel.change(cx, |terminals| terminals.unsplit(&group)));
        self.with_layout(window, cx, |layout| layout.show(TERMINAL_TOOL_WINDOW, true));
        self.with_layout(window, cx, |layout| layout.focus(TERMINAL_TOOL_WINDOW));
    }

    /// Close Terminal Session: the active tab closes, after Terminate / Cancel when its session is
    /// running a command: a program of its own, or a shell whose foreground process is another one
    /// (the backend tells). A session that is not running closes without a question.
    fn close_terminal_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.terminal_panel() else { return };
        let panel = panel.read(cx);
        let terminals = panel.terminals();
        let Some(tab) = terminals.active_tab().map(str::to_owned) else { return };
        let title = terminals.title(&tab).unwrap_or_default();
        let session = terminals.active_session();
        let running = session.is_some_and(|session| session.state.status == SessionStatus::Running);
        let program = session.and_then(|session| session.command.as_ref()).is_some_and(|command| !command.shell);
        let terminal = panel.active_view().and_then(|view| view.read(cx).id()).filter(|_| running);
        let ask_backend = terminal.filter(|_| !program);
        let busy_without_asking = terminal.is_some() && program;
        let call = ask_backend.and_then(|id| {
            let backend = cx.try_global::<Backend>()?;
            Some(backend.with_host(move |host| async move { ide_host::ops::term::is_busy(&host, id) }))
        });
        cx.spawn_in(window, async move |this, cx| {
            let busy = match call {
                // A session the backend no longer knows has ended: nothing to ask about.
                Some(call) => matches!(call.await, Ok(Ok(true))),
                None => busy_without_asking,
            };
            this.update_in(cx, |workbench, window, cx| {
                workbench.end_terminal_tab(&tab, &title, busy, window, cx);
            })
        })
        .detach();
    }

    /// Closes tab `tab`, asking first whether to terminate what it runs when `busy`.
    fn end_terminal_tab(&mut self, tab: &str, title: &str, busy: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !busy {
            return self.close_terminal_tab(tab, window, cx);
        }
        let question = cx
            .update_default_global::<Localization, _>(|localization, _| terminate_confirmation(title, &localization.0));
        let request =
            ConfirmRequest { title: question.title, message: question.message, ok_label: question.ok, danger: false };
        let answer = self.show_prompt_dialog(move |window, cx| PromptDialog::confirm(request, window, cx), window, cx);
        let tab = tab.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            // A dialog closed without an answer (the window went) ends nothing.
            if answer.await.is_ok_and(|answer| answer.confirmed()) {
                this.update_in(cx, |workbench, window, cx| workbench.close_terminal_tab(&tab, window, cx))?;
            }
            anyhow::Ok(())
        })
        .detach();
    }

    /// Closes tab `tab` and ends its session. The last tab closing hides the tool window (the panel
    /// says so); otherwise the keyboard goes to the tab that shows.
    fn close_terminal_tab(&mut self, tab: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.terminal_panel() else { return };
        let left = panel.update(cx, |panel, cx| {
            panel.close_tab(tab, cx);
            !panel.terminals().tabs().is_empty()
        });
        if left {
            self.with_layout(window, cx, |layout| layout.focus(TERMINAL_TOOL_WINDOW));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::plugins_window::test_support::{program, trusted_project};
    use super::super::tests::{click_on, run_action, start, wait_long};
    use super::*;
    use crate::platform::actions::execute_action_or_throw;
    use crate::platform::command_line::quote_path;
    use crate::project_tree::ProjectTree;
    use crate::terminal::store::GroupLayout;
    use crate::terminal::view::TerminalView;
    use gpui_kit::{Bounds, Focusable, Pixels, TestAppContext, VisualTestContext};
    use ide_workspace::settings::SettingsScope;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    /// A plain shell for the tests: `/bin/sh` reading a script, so neither interactive nor a login
    /// shell, with no startup file and no history; it runs each line typed into it.
    const PLAIN_SHELL: &str = "#!/bin/sh\nwhile IFS= read -r line; do eval \"$line\"; done\n";

    /// Makes [`PLAIN_SHELL`], at `<home>/bin/plainsh`, the shell of every terminal the test starts
    /// (on top of the home of its own `start` gives them, with `HISTFILE=/dev/null`). Returns its
    /// path.
    fn plain_shells(home: &Path, cx: &mut VisualTestContext) -> PathBuf {
        let bin = home.join("bin");
        program(&bin, "plainsh", PLAIN_SHELL);
        let shell = bin.join("plainsh");
        let line = quote_path(&shell.to_string_lossy());
        cx.update(|_, cx| {
            settings::set("", SettingsScope::Global, "terminal.shellPath", &json!(line), cx).expect("the shell is set");
            settings::set("", SettingsScope::Global, "terminal.shellIntegration", &json!(false), cx)
                .expect("shell integration is off");
        });
        shell
    }

    fn text_of(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    /// Selects `path` in the tree and gives the tree the keyboard.
    fn select_in_tree(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
        let tree: Entity<ProjectTree> = workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
        let at = text_of(path);
        wait_long(cx, "the project to be listed", |cx| tree.read_with(cx, |tree, _| tree.is_loaded(tree.root())));
        assert!(tree.update(cx, |tree, cx| tree.reveal_path(&at, cx)), "{at} is in the project");
        wait_long(cx, "the row to be selected", |cx| {
            tree.read_with(cx, |tree, _| tree.selected_path() == Some(at.as_str()))
        });
        cx.update(|window, cx| {
            let focus = tree.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    fn close_project(cx: &mut VisualTestContext) {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    fn panel_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
        workbench.read_with(cx, |w, _| w.terminal_panel()).expect("the Terminal is shown")
    }

    fn titles(panel: &Entity<TerminalPanel>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
        panel.read_with(cx, |panel, _| panel.tab_titles())
    }

    fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, _| view.emulator().lines().join("\n"))
    }

    fn active_view(panel: &Entity<TerminalPanel>, cx: &mut VisualTestContext) -> Entity<TerminalView> {
        panel.read_with(cx, |panel, _| panel.active_view().cloned()).expect("a tab shows")
    }

    /// Types `line` and Enter into the terminal that has the keyboard.
    fn type_line(line: &str, cx: &mut VisualTestContext) {
        cx.simulate_input(line);
        cx.simulate_keystrokes("enter");
    }

    /// Where the pane of `group` is drawn.
    fn pane_bounds(group: &str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.run_until_parked();
        let selector: &'static str = Box::leak(format!("terminal-group:{group}").into_boxed_str());
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("the pane of {group} is drawn"))
    }

    /// The groups the Terminal shows, in layout order.
    fn groups(panel: &Entity<TerminalPanel>, cx: &mut VisualTestContext) -> Vec<String> {
        panel.read_with(cx, |panel, _| panel.panes().into_iter().map(|(group, _)| group).collect())
    }

    fn prompt_shows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
        workbench.read_with(cx, |w, _| w.vcs_dialog_view::<PromptDialog>()).is_some()
    }

    // Not from a spec: a predefined shell's tab is named after its program, which may end in `.exe`.
    #[test]
    fn a_shells_tab_is_named_after_its_program() {
        assert_eq!(shell_name("/bin/zsh"), "zsh");
        assert_eq!(shell_name(r"C:\Program Files\PowerShell\7\pwsh.exe"), "pwsh");
        assert_eq!(shell_name("bash"), "bash");
        let run = RunCommand::from_arg(Some(&json!({ "program": "make", "args": ["all", 3], "cwd": "" })));
        let expected = RunCommand { program: "make".to_owned(), args: vec!["all".to_owned()], name: None, cwd: None };
        assert_eq!(run, Some(expected));
        assert_eq!(RunCommand::from_arg(Some(&json!({ "args": ["x"] }))), None);
    }

    /// Open in Terminal on a folder of the Project view opens a tab named after it whose shell
    /// starts there; on a Terminal never shown before that tab is the only one.
    #[gpui_kit::test]
    async fn open_in_terminal_on_a_folder_opens_a_shell_there_named_after_it(cx: &mut TestAppContext) {
        let home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("tools/run.sh", "echo hi\n"), ("a.txt", "")]);
        plain_shells(home.path(), cx);
        select_in_tree(&workbench, &root.join("tools"), cx);
        run_action(cx, "project.openInTerminal", None);
        let panel = panel_of(&workbench, cx);
        assert_eq!(titles(&panel, cx), [("tools".to_owned(), true)]);
        let shell = active_view(&panel, cx);
        assert!(cx.update(|window, cx| shell.focus_handle(cx).is_focused(window)), "the keyboard is in the shell");
        type_line("echo at-$(basename \"$PWD\")-$((6*7))", cx);
        wait_long(cx, "the shell answers", |cx| screen(&shell, cx).contains("at-tools-42"));

        run_action(cx, "terminal.openAt", Some(json!(text_of(&root))));
        let name = root.file_name().expect("a name").to_string_lossy().into_owned();
        assert_eq!(titles(&panel, cx), [("tools".to_owned(), false), (name, true)]);
        close_project(cx);
    }

    /// Run Command in Terminal runs the program in a tab of the name given, in the project, instead
    /// of a shell; New Predefined Session opens a tab running the shell given, named after it.
    #[gpui_kit::test]
    async fn run_command_runs_its_program_in_a_tab_of_its_own_and_a_predefined_session_runs_the_shell(
        cx: &mut TestAppContext,
    ) {
        let home = start(cx);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "")]);
        let shell_path = plain_shells(home.path(), cx);
        let program = json!({
            "program": "/bin/sh",
            "args": ["-c", "echo ran-$((6*7))-in-$(basename \"$PWD\"); sleep 30"],
            "name": "Job",
        });
        run_action(cx, "terminal.runCommand", Some(program));
        let panel = panel_of(&workbench, cx);
        assert_eq!(titles(&panel, cx), [("Job".to_owned(), true)]);
        let job = active_view(&panel, cx);
        let name = root.file_name().expect("a name").to_string_lossy().into_owned();
        let expected = format!("ran-42-in-{name}");
        wait_long(cx, "the program prints", |cx| screen(&job, cx).contains(&expected));

        run_action(cx, "terminal.newPredefinedSession", Some(json!(text_of(&shell_path))));
        assert_eq!(titles(&panel, cx), [("Job".to_owned(), false), ("plainsh".to_owned(), true)]);
        let shell = active_view(&panel, cx);
        assert_ne!(shell.entity_id(), job.entity_id());
        type_line("echo pre-$((2+3))", cx);
        wait_long(cx, "the shell answers", |cx| screen(&shell, cx).contains("pre-5"));

        // Without a program there is nothing to run.
        run_action(cx, "terminal.runCommand", Some(json!({ "args": ["-c", "true"] })));
        assert_eq!(titles(&panel, cx).len(), 2);
        close_project(cx);
    }

    /// Split Right puts a new shell beside the tab, Split Down below it; each pane shows its own
    /// shell. Split and Move moves a tab of a group with more than one into a new group, and does
    /// nothing to a group's only tab. Unsplit takes a group's tabs back into the one beside it.
    #[gpui_kit::test]
    async fn the_splits_lay_their_shells_side_by_side_or_stacked_and_unsplit_joins_them(cx: &mut TestAppContext) {
        let home = start(cx);
        let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "")]);
        plain_shells(home.path(), cx);
        run_action(cx, "terminal.new", None);
        let panel = panel_of(&workbench, cx);
        let first = active_view(&panel, cx);
        run_action(cx, "terminal.splitRight", None);
        let split = groups(&panel, cx);
        assert_eq!(split.len(), 2, "{split:?}");
        assert!(matches!(panel.read_with(cx, |p, _| p.terminals().layout().clone()), GroupLayout::Split(_)));
        assert_eq!(panel.read_with(cx, |panel, _| panel.started()), 2, "both panes have a shell");
        let (left, right) = (pane_bounds(&split[0], cx), pane_bounds(&split[1], cx));
        assert!(right.origin.x > left.origin.x && right.origin.y == left.origin.y, "{left:?} {right:?}");
        let second = active_view(&panel, cx);
        assert_ne!(second.entity_id(), first.entity_id());
        assert!(cx.update(|window, cx| second.focus_handle(cx).is_focused(window)), "the new pane has the keyboard");
        type_line("echo right-$((3*3))", cx);
        wait_long(cx, "the right shell answers", |cx| screen(&second, cx).contains("right-9"));
        assert!(!screen(&first, cx).contains("right-9"), "the left shell saw none of it");

        // The new group's only tab cannot be moved out of it.
        run_action(cx, "terminal.splitAndMoveDown", None);
        assert_eq!(groups(&panel, cx), split);
        // With a second tab in it, it can: the tab goes below.
        run_action(cx, "terminal.new", None);
        let moved = panel.read_with(cx, |panel, _| panel.terminals().active_tab().map(str::to_owned)).expect("a tab");
        run_action(cx, "terminal.splitAndMoveDown", None);
        let three = groups(&panel, cx);
        assert_eq!(three.len(), 3, "{three:?}");
        let tab_group = |id: &str, cx: &mut VisualTestContext| {
            panel.read_with(cx, |panel, _| panel.terminals().tab(id).map(|tab| tab.group.clone()))
        };
        assert_eq!(tab_group(&moved, cx).as_deref(), Some(three[2].as_str()));
        let (upper, lower) = (pane_bounds(&three[1], cx), pane_bounds(&three[2], cx));
        assert!(lower.origin.y > upper.origin.y && lower.origin.x == upper.origin.x, "{upper:?} {lower:?}");

        run_action(cx, "terminal.unsplit", None);
        assert_eq!(groups(&panel, cx), split);
        assert_eq!(tab_group(&moved, cx).as_deref(), Some(split[1].as_str()), "the tab joins the group beside it");
        assert_eq!(titles(&panel, cx).len(), 3, "no tab is closed");

        run_action(cx, "terminal.splitDown", None);
        let stacked = groups(&panel, cx);
        assert_eq!(stacked.len(), 3);
        let (upper, lower) = (pane_bounds(&stacked[1], cx), pane_bounds(&stacked[2], cx));
        assert!(lower.origin.y > upper.origin.y, "{upper:?} {lower:?}");
        assert_eq!(panel.read_with(cx, |panel, _| panel.started()), 4);
        close_project(cx);
    }

    /// Close Terminal Session of a tab running a command asks whether to terminate it: Cancel keeps
    /// the tab, Terminate closes it. A shell waiting for input closes without a question, and the
    /// last tab closing hides the Terminal.
    #[gpui_kit::test]
    async fn close_session_asks_before_terminating_a_running_command_and_closes_an_idle_shell_at_once(
        cx: &mut TestAppContext,
    ) {
        let home = start(cx);
        let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "")]);
        plain_shells(home.path(), cx);
        run_action(cx, "terminal.new", None);
        let panel = panel_of(&workbench, cx);
        run_action(cx, "terminal.runCommand", Some(json!({ "program": "/bin/sleep", "args": ["30"], "name": "Nap" })));
        assert_eq!(titles(&panel, cx), [("Local".to_owned(), false), ("Nap".to_owned(), true)]);

        run_action(cx, "terminal.closeSession", None);
        wait_long(cx, "the question", |cx| prompt_shows(&workbench, cx));
        let dialog = workbench.read_with(cx, |w, _| w.vcs_dialog_view::<PromptDialog>()).expect("the question");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Process 'Nap' Is Running");
        click_on(cx, "prompt-button-cancel");
        wait_long(cx, "the question to close", |cx| !prompt_shows(&workbench, cx));
        assert_eq!(titles(&panel, cx).len(), 2, "Cancel keeps the tab");

        let nap = active_view(&panel, cx);
        cx.update(|window, cx| {
            let focus = nap.focus_handle(cx);
            window.focus(&focus, cx);
        });
        run_action(cx, "terminal.closeSession", None);
        wait_long(cx, "the question again", |cx| prompt_shows(&workbench, cx));
        click_on(cx, "prompt-button-ok");
        wait_long(cx, "the tab to close", |cx| titles(&panel, cx) == [("Local".to_owned(), true)]);

        run_action(cx, "terminal.closeSession", None);
        wait_long(cx, "the idle shell's tab to close", |cx| titles(&panel, cx).is_empty());
        assert!(!prompt_shows(&workbench, cx), "an idle shell is not asked about");
        assert!(!workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
        close_project(cx);
    }

    /// The splits need a project: without one they are not available.
    #[gpui_kit::test]
    async fn the_terminal_actions_need_a_project(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        for id in ["terminal.splitRight", "terminal.openAt", "terminal.runCommand", "terminal.unsplit"] {
            let ran = cx.update(|window, cx| execute_action_or_throw(id, None, window, cx));
            assert!(ran.is_err(), "{id} needs a project");
        }
    }
}
