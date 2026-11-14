use super::*;

/// ⌘Q is the menu bar's own key for Quit (no keymap binds it): pressed in a project window it confirms the exit,
/// and Exit, with nothing unsaved and nothing running, closes every window.
#[cfg(target_os = "macos")]
#[gpui_kit::test]
async fn cmd_q_from_the_menu_bar_confirms_the_exit_and_closes_every_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    cx.update(|window, cx| {
        window.activate_window();
        menu::sync(window, cx);
    });
    cx.run_until_parked();
    let bound = cx.update(|_, cx| cx.global::<input::Keymap>().0.primary_key_binding("app.quit").map(str::to_owned));
    assert_eq!(bound, None, "the key is the menu bar's, not the keymap's");
    cx.simulate_keystrokes("cmd-q");
    cx.run_until_parked();
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Confirm Exit"));
    click_on(cx, "prompt-button-exit");
    wait_long(cx, "every window to close", |cx| cx.windows().is_empty());
}

/// Quit with a program running asks to terminate it after the exit is confirmed; Terminate stops the program and
/// the window closes once it has ended.
#[gpui_kit::test]
async fn terminate_chosen_from_quit_stops_the_running_program_and_closes_the_window(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![program("nap", "/bin/sleep", "30")]);
    cx.update(|window, _| window.activate_window());
    let live = |store: &RunStore| store.sessions.iter().any(|s| s.config.name == "nap" && s.is_live());
    run_action(cx, "run.run", None);
    wait_long(cx, "the start of sleep", |cx| store.read_with(cx, |store, _| live(store)));
    // The store is released with the window: what it last said about the program is kept here.
    let running = std::rc::Rc::new(std::cell::Cell::new(true));
    let seen = running.clone();
    cx.update(|_, cx| cx.observe(&store, move |store, cx| seen.set(live(store.read(cx)))).detach());

    run_action(cx, "app.quit", None);
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Confirm Exit"));
    click_on(cx, "prompt-button-exit");
    wait_long(cx, "the question about the program", |cx| {
        app_dialog_title(&workbench, cx).as_deref() == Some("Process 'nap' Is Running")
    });
    assert!(running.get(), "the program runs while the question is asked");
    click_on(cx, "prompt-button-terminate");
    wait_long(cx, "the window to close", |cx| cx.windows().is_empty());
    assert!(!running.get(), "the program had ended when the window closed");
}
