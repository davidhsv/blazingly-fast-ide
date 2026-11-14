use super::*;
use crate::workbench::tests::run_action;

/// The Hello fixture's manifest with Insert Date as a button of the main toolbar and of the commit message's
/// toolbar. Toolbar slots need plugin API 1.4.
fn slot_menus(text: &str) -> String {
    let engine = "engines.openidea = \"^1.0\"";
    assert!(text.contains(engine), "the fixture names the plugin API it needs");
    let text = text.replace(engine, "engines.openidea = \"^1.4\"");
    format!(
        "{text}\n\
         [[contributes.menus]]\nmenu = \"mainToolbar\"\ncommand = \"insert-date\"\n\n\
         [[contributes.menus]]\nmenu = \"commit.message\"\ncommand = \"insert-date\"\n"
    )
}

/// A trusted Git repository of `files`, committed, open in an active window, with the app's plugin host running a
/// copy of Hello whose Insert Date is a button of the main toolbar and of the commit message's toolbar.
fn open_with_slots<'a>(cx: &'a mut TestAppContext, files: &[(&str, &str)]) -> (Fixture, &'a mut VisualTestContext) {
    let home = start_with_plugins(cx);
    let hello = plugin_copy(home.path(), "hello", slot_menus);
    link(cx, &hello, &[], "insert-date");
    let folder = tempfile::tempdir().expect("a folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    for (path, text) in files {
        std::fs::write(root.join(path), text).expect("the file is written");
    }
    init_repo(&root);
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new(LABEL, window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    wait_long(cx, "the repository is read", |cx| {
        workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|vcs| vcs.available))
    });
    let home: Box<dyn std::any::Any> = Box::new(home);
    (Fixture { workbench, root, server: None, _dirs: (home, folder) }, cx)
}

/// How many digits the editor's text holds: Insert Date adds eight or more.
fn digits(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |editor, _| editor.text().chars().filter(char::is_ascii_digit).count())
}

/// A plugin's `mainToolbar` entry is a button of the window's title bar that runs its command on the editor.
#[gpui_kit::test]
async fn a_plugins_main_toolbar_entry_is_a_title_bar_button_that_runs_its_command(cx: &mut TestAppContext) {
    let (f, cx) = open_with_slots(cx, &[("notes.txt", "one two\n")]);
    let editor = f.open_file("notes.txt", cx);
    let insert = plugin_action_id(HELLO, "insert-date");
    assert_eq!(digits(&editor, cx), 0);
    let button: &'static str = Box::leak(format!("slot:mainToolbar:{insert}").into_boxed_str());
    wait_long(cx, "the main toolbar's button", |cx| cx.debug_bounds(button).is_some());
    click(cx, button.to_owned());
    wait_long(cx, "the plugin inserts the date", |cx| digits(&editor, cx) >= 8);
}

/// A plugin's `commit.message` entry is a button of the Commit window's message toolbar that runs its command.
#[gpui_kit::test]
async fn a_plugins_commit_message_entry_is_a_button_of_the_commit_message_toolbar(cx: &mut TestAppContext) {
    let (f, cx) = open_with_slots(cx, &[("notes.txt", "one two\n")]);
    let editor = f.open_file("notes.txt", cx);
    let insert = plugin_action_id(HELLO, "insert-date");
    let button: &'static str = Box::leak(format!("slot:commit.message:{insert}").into_boxed_str());
    cx.run_until_parked();
    assert!(cx.debug_bounds(button).is_none(), "the Commit window is closed: there is no button");
    run_action(cx, "toolwindow.commit", None);
    wait_long(cx, "the commit message's button", |cx| cx.debug_bounds(button).is_some());
    click(cx, button.to_owned());
    wait_long(cx, "the plugin inserts the date", |cx| digits(&editor, cx) >= 8);
}
