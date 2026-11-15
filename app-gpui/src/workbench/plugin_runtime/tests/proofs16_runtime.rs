//! Commands of running plugin code reaching the window: the window's actions run for a plugin,
//! the focused editor's selections, and a log tab opened by the code.

use super::*;

/// What the command running on `running` answered, once it ends.
fn answered(
    running: std::thread::JoinHandle<IdeResult<Option<String>>>,
    cx: &mut VisualTestContext,
) -> IdeResult<Option<String>> {
    wait(cx, "the command ends", |_| running.is_finished());
    running.join().expect("the command ran")
}

/// A plugin command that runs one of the window's own actions through the host has it run in its window
/// and gets an answer without a value; an id that is neither an action nor a plugin's command is not found.
#[gpui_kit::test]
async fn executes_host_command_requests_through_actions_and_answers_an_unknown_id_as_not_found(
    cx: &mut TestAppContext,
) {
    let home = start(cx);
    let folder = plugin_copy(home.path(), "api14", str::to_owned);
    link(cx, &folder, &["ide.commands"], "execute");
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    assert!(workbench.read_with(cx, |w, _| w.settings_plugins_page().is_none()));

    let ran = answered(run_in_window(cx, &root, "api14", "execute", Some("plugins.settings")), cx);
    assert_eq!(ran.expect("the action ran"), None);
    wait(cx, "Settings › Plugins opens", |cx| workbench.read_with(cx, |w, _| w.settings_plugins_page().is_some()));

    let missing = answered(run_in_window(cx, &root, "api14", "execute", Some("test.noSuchCommand")), cx);
    assert!(missing.is_err(), "nothing runs an unknown id: {missing:?}");
}

/// Whether `text` is a date as `yyyy-mm-dd`.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| if i == 4 || i == 7 { *b == b'-' } else { b.is_ascii_digit() })
}

/// A plugin command run from the registry gets the focused editor's selection in UTF-16 columns: the
/// selection after an emoji (two UTF-16 units) is exactly what Insert Date replaces.
#[gpui_kit::test]
async fn runs_commands_with_the_focused_editors_utf16_selections(cx: &mut TestAppContext) {
    const TEXT: &str = "ab\n😀cd\nend\n";
    let home = start(cx);
    let id = link(cx, &plugin_copy(home.path(), "hello", str::to_owned), &[], "insert-date");
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", TEXT)]);
    let file = root.join("a.txt");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    wait(cx, "the file opens", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file.as_path()))
    });
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    // Line 1 from its start to after `d`: the emoji, `c` and `d` are four UTF-16 units.
    editor.update(cx, |editor, cx| {
        let (from, to) = (editor.offset_of(1, 0), editor.offset_of(1, 4));
        editor.select(from, to, cx);
    });
    run_action(cx, &plugin_action_id(&id, "insert-date"), None);
    wait(cx, "the plugin edits the text", |cx| editor.read_with(cx, |e, _| e.text() != TEXT));
    let text = editor.read_with(cx, |e, _| e.text());
    let lines: Vec<&str> = text.split('\n').collect();
    assert_eq!((lines[0], lines[2]), ("ab", "end"), "{text:?}");
    assert!(is_date(lines[1]), "the selection, and only it, is replaced: {text:?}");
}

/// A log tab opened by plugin code stays open while another tab is shown: back on it, it is the same
/// view, with its switch still on and the program's output still there.
#[cfg(unix)]
#[gpui_kit::test]
async fn keeps_a_plugin_log_tab_and_its_program_while_another_tab_is_shown(cx: &mut TestAppContext) {
    const TAB: &str = "Probe logs (code)";
    let home = start(cx);
    programs_in(Path::new("/bin"), cx);
    let folder = plugin_copy(home.path(), "web-log", str::to_owned);
    let id = link(cx, &folder, &["process:bash"], "code");
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    let active = |name: &str, cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| w.tab_titles().iter().any(|tab| tab.name == name && tab.active))
    };
    let tab = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| w.plugin_log_tabs().into_iter().next().map(|(_, view)| view))
    };
    run_action(cx, &plugin_action_id(&id, "code"), None);
    wait(cx, "the log tab opens", |cx| active(TAB, cx) && tab(cx).is_some());
    let verbose = |view: &Entity<PluginLogTab>, cx: &mut VisualTestContext| {
        view.read_with(cx, |v, _| v.lines().iter().any(|line| line.contains("DEBUG verbose output enabled")))
    };
    // How often the program started: it prints its first line once per start.
    let starts = |view: &Entity<PluginLogTab>, cx: &mut VisualTestContext| {
        view.read_with(cx, |v, _| v.lines().iter().filter(|line| line.contains("INFO probe line 1 ")).count())
    };
    run_action(cx, TOGGLE_ARGUMENT, Some(serde_json::json!({ "id": "verbose", "checked": true })));
    wait(cx, "the program runs verbose", |cx| tab(cx).is_some_and(|view| verbose(&view, cx)));
    // The workbench holds the tab weakly: only the editor area keeps it alive, not this test.
    let (view_id, started) = {
        let view = tab(cx).expect("the log tab");
        (view.entity_id(), starts(&view, cx))
    };

    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("a.txt"), window, cx));
    wait(cx, "the file's tab is shown", |cx| active("a.txt", cx));
    assert!(!active(TAB, cx));
    let kept = tab(cx).expect("the log tab is still open while hidden");
    assert_eq!(kept.entity_id(), view_id, "the same view");
    drop(kept);

    run_action(cx, "editor.nextTab", None);
    wait(cx, "the log tab is shown again", |cx| active(TAB, cx));
    let shown = tab(cx).expect("the log tab");
    assert_eq!(shown.entity_id(), view_id, "not made again");
    assert_eq!(shown.read_with(cx, |v, _| v.toggles()), [("verbose".to_owned(), "Verbose".to_owned(), true)]);
    assert!(verbose(&shown, cx), "the output is still there");
    assert_eq!(starts(&shown, cx), started, "the program was not started again");
}
