use super::*;
use crate::workbench::title_bar::IDE_MODE_ITEM;
use ide_core::settings::IdeMode;

/// Whether `ide.toggleMode` shows as pressed in the window (the title bar button's pressed state).
fn toggled(cx: &mut VisualTestContext) -> Option<bool> {
    cx.update(|window, cx| {
        let action = cx.global::<ActionRegistry>().get("ide.toggleMode").expect("registered");
        action.is_toggled(window, cx)
    })
}

/// Whether the title bar lists the IDE mode switch among the entries it draws.
fn lists_the_switch(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let bar = workbench.read_with(cx, |w, _| w.title_bar().clone());
    let (left, right) = cx.update(|window, cx| bar.read(cx).shown(window, cx));
    left.iter().chain(&right).any(|id| id == IDE_MODE_ITEM)
}

/// A project opens in Blaze mode with the title bar's switch drawn unpressed; a click on it switches
/// the global `ide.mode` to Deep (full), and the switch shows as pressed.
#[gpui_kit::test]
async fn defaults_to_blaze_mode_and_switches_the_global_value_from_the_title_bar(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    cx.run_until_parked();
    assert_eq!(workbench.read_with(cx, |w, cx| w.ide_mode(cx)), IdeMode::Light);
    assert!(lists_the_switch(&workbench, cx));
    assert!(cx.debug_bounds("title:app.ideMode").is_some(), "the switch is drawn");
    assert_eq!(toggled(cx), Some(false));

    click_on(cx, "title:app.ideMode");
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.ide_mode(cx)) == IdeMode::Full);
    assert_eq!(cx.update(|_, cx| crate::platform::settings::global(cx).ide.mode), IdeMode::Full);
    assert_eq!(toggled(cx), Some(true));
    assert!(cx.debug_bounds("title:app.ideMode").is_some(), "the switch stays drawn");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// The switch is not drawn in a window without a project, nor in one that edits a loose file.
#[gpui_kit::test]
async fn is_hidden_without_a_project_and_in_light_edit_windows(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let file = folder.path().canonicalize().expect("the folder exists").join("notes.txt");
    std::fs::write(&file, "notes\n").expect("the file is written");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.run_until_parked();
    assert!(!lists_the_switch(&workbench, cx));
    assert!(cx.debug_bounds("title:app.ideMode").is_none(), "no switch without a project");

    workbench.update_in(cx, |w, window, cx| w.open_file(file.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    cx.run_until_parked();
    assert!(context_is_on(cx, "lightEdit"));
    assert!(!lists_the_switch(&workbench, cx));
    assert!(cx.debug_bounds("title:app.ideMode").is_none(), "no switch in a single-file window");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
