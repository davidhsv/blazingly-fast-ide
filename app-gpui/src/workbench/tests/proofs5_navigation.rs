use super::*;
use crate::platform::actions::execute_action_or_throw;

/// The file bookmarks of the window: (file name, is a folder).
fn file_marks(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    workbench.read_with(cx, |w, _| {
        let name = |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
        w.bookmarks().data().paths.iter().map(|mark| (name(&mark.path), mark.dir)).collect()
    })
}

/// The mnemonic chooser shown in the window's popup slot, while it is.
fn mnemonic_chooser(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<MnemonicChooser>> {
    workbench.read_with(cx, |w, _| w.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok()))
}

/// Toggle File Bookmark without targets bookmarks the active editor's file, and without an editor it bookmarks
/// nothing; with the targets the Project view's menu gives it bookmarks a folder; again on the file it removes
/// the file's bookmark. The file bookmarks are stored with the project.
#[gpui_kit::test]
async fn toggle_file_bookmark_without_targets_bookmarks_the_active_editors_file(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    run_action(cx, "bookmarks.toggleFileBookmark", None);
    assert!(file_marks(&workbench, cx).is_empty(), "without an editor there is no file to bookmark");

    let alpha = root.join("src/alpha.rs");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(alpha.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == alpha.as_path())));
    run_action(cx, "bookmarks.toggleFileBookmark", None);
    assert_eq!(file_marks(&workbench, cx), [("alpha.rs".to_owned(), false)]);
    let stored =
        cx.update(|_, cx| cx.global::<UiStorage>().get(&lists_key(&root.to_string_lossy())).map(str::to_owned));
    assert!(stored.is_some_and(|stored| stored.contains("alpha.rs")), "the file bookmark is stored with the project");

    let folder = serde_json::json!({ "targets": [{ "path": root.join("src").to_string_lossy(), "dir": true }] });
    run_action(cx, "bookmarks.toggleFileBookmark", Some(folder));
    let marks = file_marks(&workbench, cx);
    assert!(marks.contains(&("src".to_owned(), true)) && marks.contains(&("alpha.rs".to_owned(), false)), "{marks:?}");

    run_action(cx, "bookmarks.toggleFileBookmark", None);
    assert_eq!(file_marks(&workbench, cx), [("src".to_owned(), true)], "toggling the bookmarked file removes it");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// ⌥F3 opens the mnemonic chooser for the caret's line, and the digit typed there gives the line a bookmark with
/// that mnemonic and closes the chooser; on that line the chooser offers to change it. Without an editor the action
/// does not run and ⌥F3 opens nothing.
#[gpui_kit::test]
async fn toggle_bookmark_with_mnemonic_gives_the_caret_line_the_typed_digit(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    cx.update(|window, _| window.activate_window());
    let unavailable =
        cx.update(|window, cx| execute_action_or_throw("navigate.toggleBookmarkWithMnemonic", None, window, cx));
    assert!(unavailable.is_err(), "the action needs a text editor");
    cx.simulate_keystrokes("alt-f3");
    assert!(mnemonic_chooser(&workbench, cx).is_none(), "⌥F3 opens nothing without an editor");

    let alpha = root.join("src/alpha.rs");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(alpha.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == alpha.as_path())));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    editor.update(cx, |editor, cx| {
        let at = editor.offset_of(2, 0);
        editor.select(at, at, cx);
    });
    cx.run_until_parked();

    cx.simulate_keystrokes("alt-f3");
    let chooser = mnemonic_chooser(&workbench, cx).expect("⌥F3 opens the chooser");
    assert_eq!(chooser.read_with(cx, |chooser, _| chooser.title()), "Add Mnemonic Bookmark");
    cx.simulate_keystrokes("5");
    assert!(mnemonic_chooser(&workbench, cx).is_none(), "choosing a mnemonic closes the chooser");
    let marks: Vec<(u32, Option<char>)> =
        workbench.read_with(cx, |w, _| w.bookmarks().data().items.iter().map(|b| (b.line, b.mnemonic)).collect());
    assert_eq!(marks, [(2, Some('5'))]);

    run_action(cx, "navigate.toggleBookmarkWithMnemonic", None);
    let chooser = mnemonic_chooser(&workbench, cx).expect("the action opens the chooser");
    let offered = chooser.read_with(cx, |chooser, _| (chooser.title(), chooser.current_mnemonic()));
    assert_eq!(offered, ("Change Mnemonic", Some('5')));
    cx.simulate_keystrokes("escape");
    assert!(mnemonic_chooser(&workbench, cx).is_none());
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
