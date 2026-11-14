//! A project's own plugins belong to its windows: another window, trusted or restricted, shows
//! none of their contributions and cannot run their commands, while installed plugins show in
//! every window.

use super::*;
use crate::platform::actions::{execute_action, is_action_enabled};
use crate::platform::context::window_context;
use gpui_kit::AnyWindowHandle;

/// A window known as `label` that opens the project at `root`, trusted or restricted beforehand.
fn project_window(
    cx: &mut TestAppContext,
    label: &'static str,
    root: &Path,
    trusted: bool,
) -> (Entity<Workbench>, AnyWindowHandle) {
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(root, trusted, false)).expect("decided");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new(label, window, cx));
    cx.update(|window, _| window.activate_window());
    let path = root.to_path_buf();
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(path, window, cx));
    wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let handle = cx.update(|window, _| window.window_handle());
    (workbench, handle)
}

/// A plain project of one file.
fn plain_project() -> (tempfile::TempDir, PathBuf) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
    (folder, root)
}

/// The window `handle`, made the active one to be driven.
fn drive(handle: AnyWindowHandle, cx: &TestAppContext) -> VisualTestContext {
    let mut cx = VisualTestContext::from_window(handle, cx);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx
}

/// Closes the window `handle` as its close button does, when it is not the last project window.
fn close_window(handle: AnyWindowHandle, cx: &TestAppContext) {
    let mut closing = drive(handle, cx);
    assert!(closing.simulate_close(), "the window closes at once");
    closing.update(|window, _| window.remove_window());
    closing.run_until_parked();
}

/// What plugin `plugin` contributes to the window of `workbench`: listed for it, its Word Count
/// command an action the window can run, the Tools menu entry, the status item, the Greetings
/// tool window and the settings page of `settings`.
fn seen(workbench: &Entity<Workbench>, plugin: &str, settings: &str, cx: &mut VisualTestContext) -> OwnPlugins {
    use super::super::super::menu;
    use super::super::super::menu_spec::DYNAMIC_PLUGIN_TOOLS;
    let action = plugin_action_id(plugin, "word-count");
    let loaded = cx.update(|window, cx| {
        crate::plugins::window_listing(window, cx)
            .is_some_and(|listed| listed.plugins.iter().any(|listed| listed.id == plugin))
    });
    let runnable = cx.update(|window, cx| {
        cx.global::<ActionRegistry>()
            .get(&action)
            .is_some_and(|registered| is_action_enabled(&registered.meta, &window_context(window, cx)))
    });
    let state = cx.update(|window, cx| menu::state(window, cx));
    let menu = state.dynamic.get(DYNAMIC_PLUGIN_TOOLS).is_some_and(|items| items.iter().any(|i| i.action == action));
    let status_id = format!("plugin.{plugin}/hello");
    let status = workbench.read_with(cx, |w, cx| w.status_bar().read(cx).model().spec(&status_id).is_some());
    let tool_window_id = format!("plugin.{plugin}.greetings");
    let tool_window =
        workbench.read_with(cx, |w, _| w.tool_windows().and_then(|layout| layout.window(&tool_window_id)).is_some());
    let settings_page = cx.update(|window, cx| {
        super::super::super::plugin_contributions::plugin_settings_pages(window, cx)
            .iter()
            .any(|page| page.id == format!("plugins.{settings}"))
    });
    OwnPlugins { loaded, action: runnable, menu, status, tool_window, settings_page }
}

/// Runs the project plugin's Word Count in the window driven by `cx`: whether the window ran it,
/// and whether the plugin's code was loaded after a second of plugin events.
fn run_word_count(cx: &mut VisualTestContext) -> (bool, bool) {
    let ran = cx.update(|window, cx| execute_action(WORD_COUNT, None, window, cx));
    let_events_flow(cx);
    let host = cx.update(|_, cx| cx.global::<Plugins>().host.clone());
    (ran, host.info_of("hello").is_ok_and(|info| info.status == PluginStatus::Active))
}

/// A second trusted project's window shows none of the first project's own plugins: not listed,
/// no action, no menu entry, status item, tool window or settings page; its command does not run
/// there. The first window keeps them all.
#[gpui_kit::test]
async fn a_trusted_projects_own_plugins_contribute_nothing_to_another_trusted_projects_window(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let (_own, root) = project_with_own_plugins(&home);
    let (first, a) = project_window(cx, "w1", &root, true);
    agree_to_own_plugins(&root, &mut drive(a, cx));
    wait_long(&mut drive(a, cx), "the project's plugins contribute", |cx| {
        seen(&first, "hello", "settings-pages", cx) == ALL
    });
    let (_other, other_root) = plain_project();
    let (second, b) = project_window(cx, "w2", &other_root, true);
    let mut there = drive(b, cx);
    let_events_flow(&mut there);
    let shown = seen(&second, "hello", "settings-pages", &mut there);
    // Whether the window ran the command, and whether that loaded the plugin's code.
    assert_eq!((shown, run_word_count(&mut there)), (NONE, (false, false)));
    assert_eq!(run_word_count(&mut drive(a, cx)), (true, true), "the project's own window runs it");
    assert_eq!(seen(&first, "hello", "settings-pages", &mut drive(a, cx)), ALL);
    cx.update(|cx| {
        let host = &cx.global::<Backend>().host;
        host.close_project("w1");
        host.close_project("w2");
    });
}

/// A window in Restricted Mode shows none of another trusted project's own plugins and cannot run
/// their commands.
#[gpui_kit::test]
async fn a_trusted_projects_own_plugins_contribute_nothing_to_a_restricted_window(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let (_own, root) = project_with_own_plugins(&home);
    let (first, a) = project_window(cx, "w1", &root, true);
    agree_to_own_plugins(&root, &mut drive(a, cx));
    wait_long(&mut drive(a, cx), "the project's plugins contribute", |cx| {
        seen(&first, "hello", "settings-pages", cx) == ALL
    });
    let (_other, other_root) = plain_project();
    let (second, b) = project_window(cx, "w2", &other_root, false);
    let mut there = drive(b, cx);
    assert!(second.read_with(&there, |w, cx| w.project_restricted(cx)));
    let_events_flow(&mut there);
    let shown = seen(&second, "hello", "settings-pages", &mut there);
    // Whether the window ran the command, and whether that loaded the plugin's code.
    assert_eq!((shown, run_word_count(&mut there)), (NONE, (false, false)));
    assert_eq!(run_word_count(&mut drive(a, cx)), (true, true), "the project's own window runs it");
    cx.update(|cx| {
        let host = &cx.global::<Backend>().host;
        host.close_project("w1");
        host.close_project("w2");
    });
}

/// Closing another window leaves a project's own plugins with its window; closing its window
/// unloads them, and the other windows keep their project and the installed plugin, which every
/// window shows.
#[gpui_kit::test]
async fn closing_a_window_unloads_only_its_own_projects_plugins(cx: &mut TestAppContext) {
    const GREETER: &str = "greeter";
    let home = start_with_plugins(cx);
    let (_own, root) = project_with_own_plugins(&home);
    let installed = plugin_copy(home.path(), "hello", |text| text.replacen("id = \"hello\"", "id = \"greeter\"", 1));
    link(cx, &installed, &[], "word-count");
    let installed_in = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        let shown = seen(workbench, GREETER, GREETER, cx);
        shown.loaded && shown.action && shown.status && shown.tool_window
    };
    let (first, a) = project_window(cx, "w1", &root, true);
    agree_to_own_plugins(&root, &mut drive(a, cx));
    wait_long(&mut drive(a, cx), "the project's plugins contribute", |cx| {
        seen(&first, "hello", "settings-pages", cx) == ALL && installed_in(&first, cx)
    });
    let (_second_folder, second_root) = plain_project();
    let (second, b) = project_window(cx, "w2", &second_root, true);
    wait_long(&mut drive(b, cx), "the installed plugin shows in the second window", |cx| installed_in(&second, cx));
    // The window's view is released with the window once nothing else holds it.
    drop(second);
    close_window(b, cx);
    let host = cx.update(|cx| cx.global::<Plugins>().host.clone());
    let projects = cx.update(|cx| cx.global::<Backend>().host.clone());
    let mut kept = drive(a, cx);
    wait_long(&mut kept, "the closed window's project is closed", |_| projects.project("w2").is_err());
    let_events_flow(&mut kept);
    assert_eq!(seen(&first, "hello", "settings-pages", &mut kept), ALL, "the first window keeps its plugins");

    let (_third_folder, third_root) = plain_project();
    let (third, c) = project_window(cx, "w3", &third_root, true);
    wait_long(&mut drive(c, cx), "the installed plugin shows in the third window", |cx| installed_in(&third, cx));
    drop(first);
    close_window(a, cx);
    let mut remaining = drive(c, cx);
    wait_long(&mut remaining, "the first project's plugins are unloaded", |_| {
        !host.list(Some(&root)).plugins.iter().any(|plugin| plugin.id == "hello")
    });
    assert!(projects.project("w1").is_err(), "the closed window's project is closed");
    assert!(!remaining.update(|_, cx| cx.global::<ActionRegistry>().contains(WORD_COUNT)), "its command is gone");
    let_events_flow(&mut remaining);
    assert!(installed_in(&third, &mut remaining), "the remaining window keeps the installed plugin");
    assert!(projects.project("w3").is_ok(), "and its project");
    projects.close_project("w3");
}
