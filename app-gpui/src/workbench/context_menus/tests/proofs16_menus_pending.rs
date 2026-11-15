use super::*;

/// A window editing a file of its own, with no project: the gutter's Add Bookmark row is drawn as a row
/// that can be chosen, as it is in a project's window.
#[gpui_kit::test]
async fn the_gutters_add_bookmark_row_is_not_greyed_out_without_a_project(cx: &mut TestAppContext) {
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
    assert!(!disabled(&menu, "Add Bookmark", cx));
}
