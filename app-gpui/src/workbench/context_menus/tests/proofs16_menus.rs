use super::*;

/// The Hello fixture's manifest with an entry in the editor tab's menu.
fn hello_in_the_tab_menu(text: &str) -> String {
    let engine = "engines.openidea = \"^1.0\"";
    assert!(text.contains(engine), "the fixture names the plugin API it needs");
    let text = text.replace(engine, "engines.openidea = \"^1.4\"");
    format!("{text}\n[[contributes.menus]]\nmenu = \"editorTab.context\"\ncommand = \"insert-date\"\n")
}

/// A plugin's entry in the editor tab's menu is offered there and, chosen, runs its command on the file
/// of the tab: the plugin inserts the date into it.
#[gpui_kit::test]
async fn a_plugins_editor_tab_entry_is_offered_in_the_tabs_menu_and_runs_its_command(cx: &mut TestAppContext) {
    use crate::workbench::TabCommand;
    let home = start_with_plugins(cx);
    let hello = plugin_copy(home.path(), "hello", hello_in_the_tab_menu);
    link(cx, &hello, &[], "insert-date");
    let folder = tempfile::tempdir().expect("a folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    let file = root.join("notes.txt");
    std::fs::write(&file, "one two\n").expect("the file is written");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new(LABEL, window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |w, window, cx| w.open_project(root.clone(), window, cx));
    wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    wait_long(cx, "the file opens", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file.as_path()))
    });
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let tab = workbench.read_with(cx, |w, _| {
        let tabs = w.tabs().expect("a project is open");
        tabs.pane(&tabs.active_pane).expect("the active pane").tabs[0].id.clone()
    });
    let bounds = cx.debug_bounds(Box::leak(format!("tab:{tab}").into_boxed_str())).expect("the tab is drawn");
    right_click(cx, bounds.center());
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<TabCommand>()).expect("the tab's menu is open");
    assert!(root_rows(&menu, cx).contains(&"Hello: Insert Date".to_owned()), "{:?}", root_rows(&menu, cx));
    assert!(!disabled(&menu, "Hello: Insert Date", cx));
    choose(&menu, &["Hello: Insert Date"], cx);
    wait_long(cx, "the plugin inserts the date", |cx| {
        editor.read_with(cx, |editor, _| editor.text().chars().filter(char::is_ascii_digit).count() >= 8)
    });
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<TabCommand>()).is_none(), "the menu closes");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));
}

/// Send to Agent is left out of the editor's menu of a virtual document (a class file of a library), and
/// offered for a file of the disk.
#[test]
fn the_send_to_agent_row_is_left_out_for_a_virtual_document() {
    let facts = Rc::new(GroupFacts { project_open: true, ..GroupFacts::default() });
    let rows = |path: &str| {
        let paths = [path.to_owned()];
        let ctx = MenuGroupContext { paths: &paths, dirs: &[false], text: Some(""), line: Some(0) };
        let (rows, failed) = native_groups(&facts).items(ContextMenu::Editor, &ctx, || MenuItem::Separator);
        assert!(failed.is_empty());
        labels(&rows)
    };
    assert!(!rows("jdt://contents/A.class").contains(&"agents.sendToAgent".to_owned()));
    assert!(rows("/p/A.java").contains(&"agents.sendToAgent".to_owned()));
}

/// A window editing a file of its own, with no project: a right click in the gutter opens its menu with
/// Add Bookmark first, and choosing it bookmarks nothing.
#[gpui_kit::test]
async fn the_gutters_menu_without_a_project_offers_add_bookmark_and_bookmarks_nothing(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a folder");
    let file = folder.path().canonicalize().expect("the folder exists").join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").expect("the file is written");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new(LABEL, window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |w, window, cx| w.open_file(file.clone(), window, cx));
    wait_long(cx, "the file opens", |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    cx.run_until_parked();
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let line_start = editor.read_with(cx, |editor, _| editor.offset_of(1, 0));
    let text_at = at_offset(&editor, line_start, cx);
    let gutter = point(text_at.x - px(crate::editor::view::FOLD_GUTTER) - px(12.0), text_at.y);
    right_click(cx, gutter);
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<GutterCommand>()).expect("the gutter's menu is open");
    assert_eq!(shown(&menu, cx)[0][0], "Add Bookmark");
    choose(&menu, &["Add Bookmark"], cx);
    cx.run_until_parked();
    let path = file.to_string_lossy().into_owned();
    assert!(!workbench.read_with(cx, |w, _| has_bookmark(&w.bookmarks().data().items, &path, 1)));
}

/// A window editing a file of its own, with no project: a right click in the text opens the editor's
/// menu, and its Copy puts the selected text on the clipboard.
#[gpui_kit::test]
async fn the_editors_menu_without_a_project_opens_on_the_text_and_copies(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a folder");
    let file = folder.path().canonicalize().expect("the folder exists").join("a.txt");
    std::fs::write(&file, "hello world\n").expect("the file is written");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new(LABEL, window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |w, window, cx| w.open_file(file.clone(), window, cx));
    wait_long(cx, "the file opens", |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    cx.run_until_parked();
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    editor.update(cx, |editor, cx| editor.select(0, 5, cx));
    let at = at_offset(&editor, 2, cx);
    right_click(cx, at);
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<EditorMenuCommand>()).expect("the editor's menu is open");
    let rows = root_rows(&menu, cx);
    assert!(rows.contains(&"Copy".to_owned()) && rows.contains(&"Paste".to_owned()), "{rows:?}");
    choose(&menu, &["Copy"], cx);
    wait_long(cx, "the text is copied", |cx| {
        cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())).as_deref() == Some("hello")
    });
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<EditorMenuCommand>()).is_none(), "the menu closes");
}
