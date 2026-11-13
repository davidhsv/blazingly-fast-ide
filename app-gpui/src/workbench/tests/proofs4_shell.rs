use super::*;
use crate::terminal::panel::TerminalPanel;
use crate::workbench::menu::{MenuBar, MenuCommand, MenuNode};
use crate::workbench::welcome_model::WelcomeCommand;

/// Whether the process `pid` is still running (an exited one waiting to be reaped is not).
fn process_is_running(pid: &str) -> bool {
    let output = std::process::Command::new("ps").args(["-o", "stat=", "-p", pid]).output().expect("ps runs");
    let state = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    output.status.success() && !state.is_empty() && !state.starts_with('Z')
}

/// The Open Recent items of the menu bar: their path and label.
fn recent_items(nodes: &[MenuNode]) -> Vec<(Option<String>, String)> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            MenuNode::Submenu { children, .. } => out.extend(recent_items(children)),
            MenuNode::Item { action, arg, label, .. } if action == "file.openRecentProject" => {
                out.push((arg.clone(), label.clone()));
            }
            _ => {}
        }
    }
    out
}

/// Close Terminal Session ends the shell of the tab: the process that ran in it is gone.
#[gpui_kit::test]
async fn closing_a_terminal_session_ends_its_shell(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "a\n")]);
    cx.update(|window, _| window.activate_window());
    run_action(cx, "terminal.new", None);
    let panel = workbench
        .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
        .and_then(|view| view.downcast::<TerminalPanel>().ok())
        .expect("the Terminal window holds its tabs");
    let shell = panel.read_with(cx, |panel, _| panel.active_view().cloned()).expect("a tab");
    cx.update(|window, cx| {
        let focus = shell.focus_handle(cx);
        window.focus(&focus, cx);
    });
    let pid_file = root.join("shell.pid");
    cx.simulate_input(&format!("echo $$ > '{}'", pid_file.display()));
    cx.simulate_keystrokes("enter");
    let pid_of = || std::fs::read_to_string(&pid_file).ok().map(|text| text.trim().to_owned());
    wait_long(cx, "the shell to write its process id", |_| pid_of().is_some_and(|pid| pid.parse::<u32>().is_ok()));
    let pid = pid_of().expect("the process id is written");
    assert!(process_is_running(&pid), "the shell runs while its tab is open");

    // Only the panel holds the session now: closing it releases it.
    drop(shell);
    drop(panel);
    run_action(cx, "terminal.closeSession", None);
    wait_long(cx, "the shell to end", |_| !process_is_running(&pid));
}

/// New Group on a recent project of the Welcome screen stores the group with the project in it:
/// the recent projects read again from their file have it, and the list shows the project under it.
#[gpui_kit::test]
async fn a_group_named_on_the_welcome_screen_is_stored_with_its_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
    let name = root.file_name().expect("a folder name").to_string_lossy().into_owned();
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.project_root(cx).as_deref() == Some(&*root)));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
    cx.run_until_parked();
    let welcome = workbench.read_with(cx, |w, _| w.welcome().cloned()).expect("the Welcome screen shows");
    let config = cx.update(|_, cx| cx.global::<Backend>().dirs.config.clone());
    assert!(ide_workspace::RecentProjects::load(&config).groups().is_empty());

    let path = root.to_string_lossy().into_owned();
    welcome.update_in(cx, |view, window, cx| view.run(WelcomeCommand::NewGroup { path: Some(path) }, window, cx));
    cx.run_until_parked();
    cx.simulate_input("Work");
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the group to be stored", |_| {
        let groups = ide_workspace::RecentProjects::load(&config).groups();
        groups.iter().any(|group| group.name == "Work" && group.paths.iter().any(|path| path.0 == root))
    });
    let groups = ide_workspace::RecentProjects::load(&config).groups();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(welcome.read_with(cx, |view, _| view.lines()), ["[Work 1]".to_owned(), format!("  {name}")]);
}

/// Open Recent of the menu bar lists a project once it has been opened, under its name and path;
/// choosing it opens the project again.
#[gpui_kit::test]
async fn open_recent_lists_a_project_once_opened_and_choosing_it_opens_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
    let name = root.file_name().expect("a folder name").to_string_lossy().into_owned();
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    let shown = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            crate::workbench::menu::sync(window, cx);
            recent_items(cx.global::<MenuBar>().shown().expect("a bar is installed"))
        })
    };
    assert!(shown(cx).is_empty(), "nothing has been opened yet");

    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.project_root(cx).as_deref() == Some(&*root)));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
    cx.run_until_parked();
    let items = shown(cx);
    let [(arg, label)] = items.as_slice() else { panic!("one recent project: {items:?}") };
    assert_eq!(arg.as_deref(), Some(&*root.to_string_lossy()));
    assert!(label.starts_with(&name), "{label}");

    cx.dispatch_action(MenuCommand { action: "file.openRecentProject".to_owned(), arg: arg.clone() });
    cx.run_until_parked();
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.project_root(cx).as_deref() == Some(&*root)));
    assert!(workbench.read_with(cx, |w, _| w.welcome().is_none()), "the project replaces the Welcome screen");
}
