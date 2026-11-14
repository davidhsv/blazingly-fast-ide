use super::*;

/// What `command` of `plugin` answers for the window, run to its end.
fn ask(cx: &mut VisualTestContext, root: &Path, plugin: &str, command: &str, arg: Option<&str>) -> Option<String> {
    run_in_window(cx, root, plugin, command, arg).join().expect("the command ran").ok().flatten()
}

/// Opens `path` in a tab of the window and waits until its editor is the focused one.
fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let at = path.to_path_buf();
    workbench.update_in(cx, |w, window, cx| w.open_project_file(at, window, cx));
    wait(cx, "the file opens", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path))
    });
}

/// Closes the focused editor's tab and opens `path` again: the window reports the file as closed, then opened.
/// The window posts only the kinds the plugins want, which it learns after the plugins load; a test repeats
/// this until the plugin heard what it waits for.
fn reopen(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    if workbench.read_with(cx, |w, _| w.editor().is_some()) {
        run_action(cx, "editor.closeTab", None);
        wait(cx, "the tab closes", |cx| workbench.read_with(cx, |w, _| w.editor().is_none()));
    }
    open(workbench, path, cx);
}

/// A file the window opens reaches the plugin subscribed to `file-opened` for its path, which counts the events it
/// hears; once the plugin unsubscribes, opening the file again reaches it no more.
#[gpui_kit::test]
async fn a_file_the_window_opens_reaches_the_plugin_subscribed_to_file_opened(cx: &mut TestAppContext) {
    let home = start(cx);
    let folder = plugin_copy(home.path(), "path-events", str::to_owned);
    link(cx, &folder, &["events", "project.read"], "count");
    let (_folder, root, workbench, cx) = project(cx, &[("config/app.yaml", "a: 1\n")]);
    const PLUGIN: &str = "test.path-events";
    // Its first command loads the plugin, which subscribes to openings of `config/*.yaml`.
    assert_eq!(ask(cx, &root, PLUGIN, "count", None), None, "nothing heard yet");
    let file = root.join("config/app.yaml");
    wait(cx, "the plugin heard the file open", |cx| {
        let heard = ask(cx, &root, PLUGIN, "count", None).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0) > 0;
        if !heard {
            reopen(&workbench, &file, cx);
        }
        heard
    });
    ask(cx, &root, PLUGIN, "off", None);
    let before = ask(cx, &root, PLUGIN, "count", None);
    reopen(&workbench, &file, cx);
    // Two of the window's posting intervals and the host's delivery pass.
    let until = std::time::Instant::now() + Duration::from_secs(2);
    wait(cx, "two seconds", |_| std::time::Instant::now() >= until);
    assert_eq!(ask(cx, &root, PLUGIN, "count", None), before, "an unsubscribed plugin hears nothing");
}
