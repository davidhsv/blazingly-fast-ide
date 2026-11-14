use super::*;

/// A plugin command that runs an IDE action has it run in the window it was invoked from, and hears
/// how it went: the action's effect shows and the command ends with success; an action that does
/// not exist makes the command fail.
#[gpui_kit::test]
async fn a_plugin_command_runs_an_ide_action_in_its_window_and_hears_the_outcome(cx: &mut TestAppContext) {
    let home = start(cx);
    let folder = plugin_copy(home.path(), "api14", str::to_owned);
    link(cx, &folder, &["ide.commands"], "execute");
    let (_folder, root, _workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    let dark = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.global::<crate::theme::ActiveTheme>().is_dark());
    assert!(!dark(cx));

    let running = run_in_window(cx, &root, "api14", "execute", Some("view.toggleTheme"));
    wait(cx, "the command ends", |_| running.is_finished());
    let ran = running.join().expect("the command ran");
    assert!(ran.is_ok(), "the action ran and the command heard so: {ran:?}");
    assert!(dark(cx), "the action ran in the window");

    let running = run_in_window(cx, &root, "api14", "execute", Some("no.such.action"));
    wait(cx, "the command ends", |_| running.is_finished());
    let failed = running.join().expect("the command ran");
    assert!(failed.is_err(), "the command hears that the action failed: {failed:?}");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));
}
