use super::*;

/// The window is titled `<project> – <file>`, the project's name alone while no file is open, and
/// the product's name while it shows no project.
#[gpui_kit::test]
async fn is_project_file_like_idea(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.run_until_parked();
    assert_eq!(cx.window_title().as_deref(), Some("OpenIDEA"));

    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::create_dir(root.join("src")).expect("a folder");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("the file is written");
    cx.update(|_, cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("trusted");
    workbench.update_in(cx, |w, window, cx| w.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let name = root.file_name().expect("a name").to_string_lossy().into_owned();
    settle(cx, |cx| cx.window_title().as_deref() == Some(name.as_str()));

    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("src/main.rs"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    assert_eq!(cx.window_title(), Some(format!("{name} – main.rs")));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Help › About lists the version, the build with its profile, the platform, the runtime and the
/// configuration folder, in that order.
#[gpui_kit::test]
async fn lists_version_build_and_platform(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    run_action(cx, "app.about", None);
    let about = workbench.read_with(cx, |w, _| w.about_dialog()).expect("About is open");
    let info = about.update(cx, |a, cx| a.info_text(cx));
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    };
    let config = cx.update(|_, cx| cx.global::<Backend>().dirs.config.to_string_lossy().into_owned());
    let build = option_env!("OPENIDEA_BUILD").unwrap_or("dev");
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let expected = [
        "About OpenIDEA".to_owned(),
        format!("Version: {}", env!("OPENIDEA_VERSION")),
        format!("Build: {build} ({profile})"),
        format!("Platform: {os} {}", std::env::consts::ARCH),
        "Runtime: GPUI".to_owned(),
        format!("Configuration: {config}"),
    ];
    assert_eq!(info, expected.join("\n"));
    click_on(cx, "about-close");
}

/// Escape on a question over the window answers Cancel: the exit is not confirmed, the dialog
/// closes and the window stays.
#[gpui_kit::test]
async fn escape_cancels(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Confirm Exit"));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(app_dialog_title(&workbench, cx).is_none(), "the dialog closes");
    assert_eq!(cx.windows().len(), 1, "the window stays");
    assert!(cx.update(|_, cx| crate::platform::settings::global(cx).general.confirm_exit));
}

/// The Open Project question draws one Cancel first and the default button last (Cancel, New
/// Window, This Window); Enter picks the default, which opens the project in this window.
#[gpui_kit::test]
async fn orders_buttons_like_macos_idea_adds_one_cancel_and_enter_picks_the_default(cx: &mut TestAppContext) {
    let _home = start(cx);
    let other = tempfile::tempdir().expect("a temporary folder");
    let other_root = other.path().canonicalize().expect("the folder exists");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&other_root, true, false)).expect("trusted");
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    let path = serde_json::Value::String(other_root.to_string_lossy().into_owned());
    run_action(cx, "file.openProjectPath", Some(path));
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Open Project"));
    let dialog = workbench.read_with(cx, |w, _| w.app_dialog().cloned()).expect("the question is shown");
    let buttons: Vec<(String, String, bool)> = dialog.read_with(cx, |dialog, _| {
        dialog.buttons().iter().map(|b| (b.id.clone(), b.label.clone(), b.primary)).collect()
    });
    let expected = [("cancel", "Cancel", false), ("new", "New Window", false), ("same", "This Window", true)];
    let expected: Vec<(String, String, bool)> =
        expected.iter().map(|(id, label, primary)| ((*id).to_owned(), (*label).to_owned(), *primary)).collect();
    assert_eq!(buttons, expected);
    let left = |cx: &mut VisualTestContext, selector: &'static str| {
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).left()
    };
    let (cancel, new, same) =
        (left(cx, "prompt-button-cancel"), left(cx, "prompt-button-new"), left(cx, "prompt-button-same"));
    assert!(cancel < new && new < same, "drawn left to right: Cancel, New Window, This Window");

    cx.simulate_keystrokes("enter");
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.project_root(cx)).as_deref() == Some(&*other_root));
    assert_ne!(other_root, root);
    assert_eq!(cx.windows().len(), 1, "no window is added");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Help › Keymap Reference is an action of the window that shows the bound actions grouped by
/// category.
#[gpui_kit::test]
async fn renders_the_overlay(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    let registered = cx.update(|_, cx| cx.global::<ActionRegistry>().contains("help.keymapReference"));
    assert!(registered, "Keymap Reference is an action");
    run_action(cx, "help.keymapReference", None);
}
