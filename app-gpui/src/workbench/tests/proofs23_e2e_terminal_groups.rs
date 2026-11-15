//! The Terminal's tab groups driven as a user does: the tabs' menus, Split Right and Unsplit, a
//! group's tab closed with its button, ⌘W, and Close Other Tabs, with the question asked before a
//! running command is terminated.

use super::proofs18_e2e_fixtures::{click_at, click_menu_row, right_click_at};
use super::*;
use crate::terminal::panel::TerminalPanel;
use crate::terminal::panel::groups::TerminalTabCommand;
use crate::terminal::view::TerminalView;
use crate::vcs::prompt_dialogs::PromptDialog;

fn press(cx: &mut VisualTestContext, keys: &str) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

/// ⌘T on macOS, Ctrl+Shift+T elsewhere: New Terminal Tab while a terminal has the keyboard.
const NEW_TAB_KEYS: &str = if cfg!(target_os = "macos") { "cmd-t" } else { "ctrl-shift-t" };

fn panel_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench
        .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
        .and_then(|view| view.downcast::<TerminalPanel>().ok())
        .expect("the Terminal window holds its tabs")
}

fn active_tab(panel: &Entity<TerminalPanel>, cx: &mut VisualTestContext) -> Option<String> {
    panel.read_with(cx, |panel, _| panel.terminals().active_tab().map(str::to_owned))
}

fn active_view(panel: &Entity<TerminalPanel>, cx: &mut VisualTestContext) -> Option<Entity<TerminalView>> {
    panel.read_with(cx, |panel, _| panel.active_view().cloned())
}

/// The tab strips of the groups, in layout order: each tab's label with whether it is its group's
/// selected tab.
fn strips(panel: &Entity<TerminalPanel>, cx: &mut VisualTestContext) -> Vec<Vec<(String, bool)>> {
    panel.read_with(cx, |panel, _| panel.headers().into_iter().map(|header| header.tabs).collect())
}

fn labels(tabs: &[(&str, bool)]) -> Vec<(String, bool)> {
    tabs.iter().map(|(label, selected)| ((*label).to_owned(), *selected)).collect()
}

fn focused(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.focus_handle(cx).is_focused(window))
}

fn bounds_of(selector: String, cx: &mut VisualTestContext) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    cx.run_until_parked();
    let selector: &'static str = Box::leak(selector.into_boxed_str());
    cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
}

fn prompt_shows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |w, _| w.vcs_dialog_view::<PromptDialog>()).is_some()
}

fn prompt_title(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let dialog = workbench.read_with(cx, |w, _| w.vcs_dialog_view::<PromptDialog>()).expect("the question");
    dialog.read_with(cx, |dialog, _| dialog.title().to_owned())
}

/// ⌥F12 opens the Terminal; returns it once its first tab's shell runs.
fn open_terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    cx.update(|window, _| window.activate_window());
    press(cx, "alt-f12");
    wait_long(cx, "the terminal's first shell", |cx| {
        workbench.read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW)).is_some()
            && active_view(&panel_of(workbench, cx), cx).is_some_and(|view| view.read_with(cx, |v, _| v.id().is_some()))
    });
    panel_of(workbench, cx)
}

/// Split Right from the menu of the tab `tab`.
fn split_right_from_the_tab_menu(workbench: &Entity<Workbench>, tab: &str, cx: &mut VisualTestContext) {
    right_click_at(cx, format!("terminal-tab:{tab}"));
    click_menu_row::<TerminalTabCommand>(workbench, 0, "Split Right", cx);
}

// e2e/toolwindows.spec.ts › Split Right gives the Terminal a tab group with its own header; Unsplit merges it back
/// ⌥F12, then Split Right from the menu of the tab Local: two tab strips, the second with its tab
/// Local (2) selected, and a header per group, the title in the first and the window's Options in the
/// second only. Unsplit from the menu of Local (2) merges the tabs into one strip, Local then
/// Local (2), under one header.
#[gpui_kit::test]
async fn split_right_gives_the_terminal_a_tab_group_with_its_own_header_unsplit_merges_it_back(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    let panel = open_terminal(&workbench, cx);
    assert_eq!(strips(&panel, cx), [labels(&[("Local", true)])]);
    let local = active_tab(&panel, cx).expect("the tab Local");

    split_right_from_the_tab_menu(&workbench, &local, cx);
    assert_eq!(strips(&panel, cx), [labels(&[("Local", true)]), labels(&[("Local (2)", true)])]);
    let headers = panel.read_with(cx, |panel, _| panel.headers());
    assert!(headers[0].title && !headers[0].actions, "{headers:?}");
    assert!(!headers[1].title && headers[1].actions, "{headers:?}");
    let first = bounds_of(format!("terminal-header:{}", headers[0].group), cx);
    let second = bounds_of(format!("terminal-header:{}", headers[1].group), cx);
    let options = bounds_of("more:terminal".to_owned(), cx).center();
    assert!(second.contains(&options) && !first.contains(&options), "Options is in the second header only");
    let local_2 = active_tab(&panel, cx).expect("the tab Local (2)");
    assert!(second.contains(&bounds_of(format!("terminal-tab:{local_2}"), cx).center()));

    right_click_at(cx, format!("terminal-tab:{local_2}"));
    click_menu_row::<TerminalTabCommand>(&workbench, 0, "Unsplit", cx);
    let merged: Vec<Vec<String>> =
        strips(&panel, cx).into_iter().map(|strip| strip.into_iter().map(|(label, _)| label).collect()).collect();
    assert_eq!(merged, [["Local", "Local (2)"]]);
    assert_eq!(panel.read_with(cx, |panel, _| panel.headers().len()), 1, "one header");
    let only = panel.read_with(cx, |panel, _| panel.headers().remove(0));
    assert!(only.title && only.actions);
    close_backend_project(cx);
}

// e2e/toolwindows.spec.ts › closing the last tab of a terminal group focuses its neighbour
/// ⌥F12, Split Right from the tab's menu, then the Close Tab button of the second group's tab: one
/// strip is left and its terminal has the keyboard; ⌘T then opens a second tab.
#[gpui_kit::test]
async fn closing_the_last_tab_of_a_terminal_group_focuses_its_neighbour(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    let panel = open_terminal(&workbench, cx);
    let local = active_tab(&panel, cx).expect("the tab Local");
    split_right_from_the_tab_menu(&workbench, &local, cx);
    assert_eq!(strips(&panel, cx).len(), 2);
    let second = active_tab(&panel, cx).expect("the second group's tab");

    click_at(cx, format!("terminal-close:{second}"));
    wait_long(cx, "the second group closes", |cx| strips(&panel, cx).len() == 1);
    assert!(!prompt_shows(&workbench, cx), "an idle shell closes without a question");
    let neighbour = active_view(&panel, cx).expect("the first group's terminal");
    assert_eq!(active_tab(&panel, cx).as_deref(), Some(local.as_str()));
    wait_long(cx, "the neighbour has the keyboard", |cx| focused(&neighbour, cx));

    press(cx, NEW_TAB_KEYS);
    assert_eq!(strips(&panel, cx), [labels(&[("Local", false), ("Local (2)", true)])]);
    close_backend_project(cx);
}

/// The Close Tab button of a shell running a command in the foreground asks Terminate / Cancel:
/// Cancel keeps the tab and its command. ⌘W in that terminal asks the same, and Terminate closes the
/// tab; the last tab closing hides the Terminal.
#[gpui_kit::test]
async fn the_close_tab_button_and_cmd_w_ask_before_terminating_a_running_command(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    let panel = open_terminal(&workbench, cx);
    let tab = active_tab(&panel, cx).expect("the tab Local");
    let shell = active_view(&panel, cx).expect("its terminal");
    let id = shell.read_with(cx, |view, _| view.id()).expect("the shell runs");
    let busy = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| ide_host::ops::term::is_busy(&cx.global::<Backend>().host, id)).unwrap_or(false)
    };
    // Job control gives the command the terminal, as an interactive shell does.
    cx.simulate_input("set -m");
    press(cx, "enter");
    cx.simulate_input("echo started-$((2*4)); sleep 30");
    press(cx, "enter");
    let screen = |cx: &mut VisualTestContext| shell.read_with(cx, |view, _| view.emulator().lines().join("\n"));
    wait_long(cx, "the command runs", |cx| screen(cx).contains("started-8") && busy(cx));

    click_at(cx, format!("terminal-close:{tab}"));
    wait_long(cx, "the question", |cx| prompt_shows(&workbench, cx));
    assert_eq!(prompt_title(&workbench, cx), "Process 'Local' Is Running");
    click_on(cx, "prompt-button-cancel");
    wait_long(cx, "the question to close", |cx| !prompt_shows(&workbench, cx));
    assert_eq!(strips(&panel, cx), [labels(&[("Local", true)])], "Cancel keeps the tab");
    assert!(busy(cx), "the command still runs");

    cx.update(|window, cx| {
        let focus = shell.focus_handle(cx);
        window.focus(&focus, cx);
    });
    press(cx, "cmd-w");
    wait_long(cx, "the question again", |cx| prompt_shows(&workbench, cx));
    assert_eq!(prompt_title(&workbench, cx), "Process 'Local' Is Running");
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "the tab to close", |cx| panel.read_with(cx, |panel, _| panel.tab_titles().is_empty()));
    wait_long(cx, "the Terminal to hide", |cx| {
        !workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW))
    });
    close_backend_project(cx);
}

/// Close Other Tabs closes the other tabs of the group in strip order, asking about the one that runs
/// a program: Cancel stops there and keeps the tabs after it; Terminate goes on, and the idle shell
/// after it closes without a question.
#[gpui_kit::test]
async fn close_other_tabs_asks_about_a_running_program_and_stops_at_cancel(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    let panel = open_terminal(&workbench, cx);
    let local = active_tab(&panel, cx).expect("the tab Local");
    run_action(
        cx,
        "terminal.runCommand",
        Some(serde_json::json!({ "program": "/bin/sleep", "args": ["30"], "name": "Nap" })),
    );
    run_action(cx, "terminal.new", None);
    let names = |cx: &mut VisualTestContext| -> Vec<String> {
        strips(&panel, cx).into_iter().flatten().map(|(label, _)| label).collect()
    };
    assert_eq!(names(cx), ["Local", "Nap", "Local (2)"]);
    wait_long(cx, "the program runs", |cx| {
        panel.read_with(cx, |panel, cx| {
            panel
                .terminals()
                .tabs()
                .iter()
                .all(|tab| panel.view_of_tab(&tab.id).is_some_and(|v| v.read(cx).id().is_some()))
        })
    });

    right_click_at(cx, format!("terminal-tab:{local}"));
    click_menu_row::<TerminalTabCommand>(&workbench, 0, "Close Other Tabs", cx);
    wait_long(cx, "the question", |cx| prompt_shows(&workbench, cx));
    assert_eq!(prompt_title(&workbench, cx), "Process 'Nap' Is Running");
    click_on(cx, "prompt-button-cancel");
    wait_long(cx, "the question to close", |cx| !prompt_shows(&workbench, cx));
    assert_eq!(names(cx), ["Local", "Nap", "Local (2)"], "Cancel stops before the tabs after it");

    right_click_at(cx, format!("terminal-tab:{local}"));
    click_menu_row::<TerminalTabCommand>(&workbench, 0, "Close Other Tabs", cx);
    wait_long(cx, "the question again", |cx| prompt_shows(&workbench, cx));
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "the other tabs to close", |cx| names(cx) == ["Local"]);
    assert!(!prompt_shows(&workbench, cx), "the idle shell closes without a question");
    close_backend_project(cx);
}
