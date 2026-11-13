use super::*;

/// `focusedDirectory` is the folder selected in the Project view while the view has the keyboard;
/// once the editor takes the keyboard the key is cleared, and it is back when the view has it again.
#[gpui_kit::test]
async fn the_focused_directory_follows_the_project_views_keyboard(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.txt", "a\n"), ("lib/b.txt", "b\n")]);
    let src = root.join("src");
    let republished = |cx: &mut VisualTestContext| {
        workbench.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    };

    select_in_tree(&workbench, &src, cx);
    republished(cx);
    assert_eq!(context_text(cx, "focusedDirectory"), Some(text_of(&src)));

    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("src/a.txt"), window, cx));
    settle(cx, |cx| {
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned());
        cx.update(|window, cx| editor.is_some_and(|editor| editor.focus_handle(cx).is_focused(window)))
    });
    republished(cx);
    assert_eq!(context_text(cx, "focusedDirectory"), None, "the editor has the keyboard");
    assert!(!context_is_on(cx, "focusedDirectory"));

    let lib = root.join("lib");
    select_in_tree(&workbench, &lib, cx);
    republished(cx);
    assert_eq!(context_text(cx, "focusedDirectory"), Some(text_of(&lib)));
    close_backend_project(cx);
}

/// `run.cmakeBuild` names the CMake build the hammer runs: with a selected configuration that
/// neither is nor runs one it is cleared; with the CMake configure step selected it is the build of
/// the same directory, and with the build selected it is that build.
#[gpui_kit::test]
async fn the_cmake_build_key_follows_the_selected_configuration(cx: &mut TestAppContext) {
    let cmake = |id: &str, args: &[&str]| ide_run::RunConfig {
        command: "cmake".to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        ..crate::run::configs::blank_config(id, id, "cmake")
    };
    let configs = vec![
        cmake("app:configure", &["-S", ".", "-B", "build"]),
        cmake("app:build", &["--build", "build"]),
        program("hello", "/bin/echo", "hello"),
    ];
    let (workbench, cx, _folders, _root, store) = run_project(cx, configs);
    let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
    let select = |cx: &mut VisualTestContext, id: &str| {
        driver.update(cx, |driver, cx| driver.select_config(Some(id), cx));
        workbench.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    };
    assert!(store.read_with(cx, |store, _| store.config("app:configure").is_some()), "CMake configurations load");

    select(cx, "hello");
    assert_eq!(context_text(cx, "run.cmakeBuild"), None);
    assert!(!context_is_on(cx, "run.cmakeBuild"));

    select(cx, "app:configure");
    assert_eq!(context_text(cx, "run.cmakeBuild").as_deref(), Some("app:build"));
    select(cx, "app:build");
    assert_eq!(context_text(cx, "run.cmakeBuild").as_deref(), Some("app:build"));
    close_backend_project(cx);
}
