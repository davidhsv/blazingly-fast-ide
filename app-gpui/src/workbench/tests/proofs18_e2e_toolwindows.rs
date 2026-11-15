use super::proofs18_e2e_fixtures::{click_at, open_from_tree};
use super::*;
use crate::terminal::panel::TerminalPanel;
use crate::terminal::view::TerminalView;

fn shown(workbench: &Entity<Workbench>, id: &str, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |w, _| w.is_tool_window_visible(id))
}

fn press(cx: &mut VisualTestContext, keys: &str) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

/// The Terminal's selected tab, once the tool window holds one.
fn terminal_view(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<TerminalView>> {
    let panel = workbench
        .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
        .and_then(|view| view.downcast::<TerminalPanel>().ok())?;
    panel.read_with(cx, |panel, _| panel.active_view().cloned())
}

fn screen(terminal: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Vec<String> {
    terminal.read_with(cx, |terminal, _| terminal.emulator().lines())
}

fn focused(terminal: &Entity<TerminalView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| terminal.focus_handle(cx).is_focused(window))
}

/// ⌘4 on macOS, Alt+4 elsewhere.
const RUN_KEYS: &str = if cfg!(target_os = "macos") { "cmd-4" } else { "alt-4" };
/// ⌘⇧F12 on macOS, Ctrl+Shift+F12 elsewhere.
const HIDE_ALL_KEYS: &str = if cfg!(target_os = "macos") { "cmd-shift-f12" } else { "ctrl-shift-f12" };

/// With the editor focused, ⌘1 gives the Project view the keyboard; again it hides it, and again shows it.
#[gpui_kit::test]
async fn cmd_1_focuses_hides_and_re_shows_the_project_tool_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    cx.update(|window, _| window.activate_window());
    open_from_tree(&workbench, &root.join("README.md"), cx);
    let tree = project_tree_of(&workbench, cx);
    let tree_focused = |cx: &mut VisualTestContext| cx.update(|window, cx| tree.focus_handle(cx).is_focused(window));
    assert!(!tree_focused(cx), "the editor has the keyboard");
    press(cx, "cmd-1");
    assert!(shown(&workbench, PROJECT_TOOL_WINDOW, cx) && tree_focused(cx), "the Project tree has the keyboard");
    press(cx, "cmd-1");
    assert!(!shown(&workbench, PROJECT_TOOL_WINDOW, cx), "the Project view is hidden");
    press(cx, "cmd-1");
    assert!(shown(&workbench, PROJECT_TOOL_WINDOW, cx), "the Project view shows again");
    close_backend_project(cx);
}

/// ⌘4 shows Run, again hides it, again shows it; Shift+Escape then hides the active tool window, Run.
#[gpui_kit::test]
async fn cmd_4_toggles_run_shift_escape_hides_the_active_tool_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    cx.update(|window, _| window.activate_window());
    press(cx, RUN_KEYS);
    assert!(shown(&workbench, RUN_TOOL_WINDOW, cx));
    press(cx, RUN_KEYS);
    assert!(!shown(&workbench, RUN_TOOL_WINDOW, cx));
    press(cx, RUN_KEYS);
    assert!(shown(&workbench, RUN_TOOL_WINDOW, cx));
    press(cx, "shift-escape");
    assert!(!shown(&workbench, RUN_TOOL_WINDOW, cx), "Shift+Escape hides Run");
    close_backend_project(cx);
}

/// ⌥F12 shows the Terminal; it is among the windows shown; ⌥F12 again hides it.
#[gpui_kit::test]
async fn alt_f12_opens_the_terminal_tool_window_and_toggles_it_closed(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    cx.update(|window, _| window.activate_window());
    press(cx, "alt-f12");
    assert!(shown(&workbench, TERMINAL_TOOL_WINDOW, cx));
    let listed = workbench.read_with(cx, |w, _| {
        w.tool_windows().is_some_and(|layout| layout.visible().ids().any(|id| id == TERMINAL_TOOL_WINDOW))
    });
    assert!(listed, "the Terminal is among the windows shown");
    press(cx, "alt-f12");
    assert!(!shown(&workbench, TERMINAL_TOOL_WINDOW, cx));
    close_backend_project(cx);
}

/// The first ⌥F12 leaves the keyboard in the terminal once its view is there; hidden with ⌥F12 and shown
/// again, the terminal has the keyboard again.
#[gpui_kit::test]
async fn the_first_alt_f12_focuses_the_terminal_input_once_the_view_mounts(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    cx.update(|window, _| window.activate_window());
    press(cx, "alt-f12");
    wait_long(cx, "the terminal's view", |cx| terminal_view(&workbench, cx).is_some());
    let terminal = terminal_view(&workbench, cx).expect("a first tab");
    wait_long(cx, "the terminal has the keyboard", |cx| focused(&terminal, cx));
    press(cx, "alt-f12");
    assert!(!shown(&workbench, TERMINAL_TOOL_WINDOW, cx));
    press(cx, "alt-f12");
    assert!(shown(&workbench, TERMINAL_TOOL_WINDOW, cx));
    wait_long(cx, "the terminal has the keyboard again", |cx| focused(&terminal, cx));
    close_backend_project(cx);
}

/// ⌥F12, then at once a click on README.md in the Project view: once the terminal's shell has started, the
/// Project tree still has the keyboard.
#[gpui_kit::test]
async fn a_terminal_view_that_loads_late_does_not_take_focus_moved_elsewhere_meanwhile(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    cx.update(|window, _| window.activate_window());
    let tree = project_tree_of(&workbench, cx);
    settle(cx, |cx| tree.read_with(cx, |tree, _| tree.rows().len() == 2));
    press(cx, "alt-f12");
    assert!(shown(&workbench, TERMINAL_TOOL_WINDOW, cx));
    click_at(cx, format!("project-row:{}", text_of(&root.join("README.md"))));
    wait_long(cx, "the terminal's shell", |cx| {
        terminal_view(&workbench, cx).is_some_and(|terminal| {
            terminal.read_with(cx, |terminal, _| terminal.id().is_some())
                && screen(&terminal, cx).iter().any(|line| !line.trim().is_empty())
        })
    });
    assert!(cx.update(|window, cx| tree.focus_handle(cx).is_focused(window)), "the Project tree keeps the keyboard");
    close_backend_project(cx);
}

/// ⌘6 shows Problems beside the Project view; ⌘⇧F12 hides every tool window, and pressed again shows both.
#[gpui_kit::test]
async fn cmd_shift_f12_hides_all_tool_windows_and_restores_them(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    cx.update(|window, _| window.activate_window());
    press(cx, if cfg!(target_os = "macos") { "cmd-6" } else { "alt-6" });
    assert!(shown(&workbench, "problems", cx) && shown(&workbench, PROJECT_TOOL_WINDOW, cx));
    press(cx, HIDE_ALL_KEYS);
    let none = workbench.read_with(cx, |w, _| w.tool_windows().is_some_and(|layout| layout.visible().is_empty()));
    assert!(none, "no tool window is shown");
    press(cx, HIDE_ALL_KEYS);
    assert!(shown(&workbench, PROJECT_TOOL_WINDOW, cx), "the Project view is back");
    assert!(shown(&workbench, "problems", cx), "Problems is back");
    close_backend_project(cx);
}

/// ⌥F12 starts a shell whose prompt shows; a click on the terminal gives it the keyboard, and `echo hello
/// from e2e` typed with Enter reaches the shell, whose answer shows on one line of its own.
#[gpui_kit::test]
async fn the_terminal_sends_typed_commands_to_the_shell_and_shows_its_output(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    cx.update(|window, _| window.activate_window());
    press(cx, "alt-f12");
    wait_long(cx, "the shell's prompt", |cx| {
        terminal_view(&workbench, cx).is_some_and(|t| screen(&t, cx).iter().any(|line| !line.trim().is_empty()))
    });
    let terminal = terminal_view(&workbench, cx).expect("a tab");
    let panel = workbench
        .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
        .and_then(|view| view.downcast::<TerminalPanel>().ok())
        .expect("the Terminal window holds its tabs");
    let group = panel.read_with(cx, |panel, _| panel.panes().first().map(|(group, _)| group.clone()));
    // Away from the terminal first, so that the click is what gives it the keyboard.
    let tree = project_tree_of(&workbench, cx);
    cx.update(|window, cx| window.focus(&tree.focus_handle(cx), cx));
    cx.run_until_parked();
    assert!(!focused(&terminal, cx));
    click_at(cx, format!("terminal-group:{}", group.expect("a tab group")));
    assert!(focused(&terminal, cx), "the click gives the terminal the keyboard");
    cx.simulate_input("echo hello from e2e");
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the shell's answer", |cx| screen(&terminal, cx).iter().any(|line| line.trim() == "hello from e2e"));
    let answers = screen(&terminal, cx).iter().filter(|line| line.trim() == "hello from e2e").count();
    assert_eq!(answers, 1, "the answer shows once");
    close_backend_project(cx);
}
