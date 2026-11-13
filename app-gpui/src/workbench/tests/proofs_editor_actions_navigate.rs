use super::*;
use gpui_kit::Modifiers;

/// Where the window's editor is: its file and caret.
fn at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(PathBuf, (u32, u32))> {
    workbench.read_with(cx, |w, cx| w.editor().map(|e| (e.read(cx).path().to_path_buf(), e.read(cx).caret_position())))
}

/// Go to Class opens Search Everywhere on Classes with the keyboard in its query: a typed name
/// lists the type and not the function that contains it, and Enter opens the type's file on it.
#[gpui_kit::test]
async fn go_to_class_finds_a_type_by_name_and_opens_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);

    run_action(cx, "navigate.gotoClass", None);
    let everywhere = popup(&workbench, cx);
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Classes);
    assert!(cx.update(|window, cx| everywhere.focus_handle(cx).is_focused(window)), "the query has the keyboard");

    // "pub struct Widget;" and "pub fn make_widget() -> Widget": only the struct is a class.
    type_query(cx, "Widg");
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| !popup.is_loading() && !popup.lines().is_empty()));
    let lines = everywhere.read_with(cx, |popup, _| popup.lines());
    assert!(lines.len() == 1 && lines[0].contains("Widget") && !lines[0].contains("make_widget"), "{lines:?}");
    cx.simulate_keystrokes("enter");
    let alpha = root.join("src/alpha.rs");
    settle(cx, |cx| at(&workbench, cx) == Some((alpha.clone(), (0, 11))));
    assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()), "choosing a result closes the popup");
    close_backend_project(cx);
}

/// Previous Bookmark goes to the bookmark before the caret in file and line order, at the start
/// of its line, and from the first one round to the last.
#[gpui_kit::test]
async fn previous_bookmark_goes_back_through_the_bookmarks_and_round_to_the_last(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    let (alpha, beta) = (root.join("src/alpha.rs"), root.join("src/beta.rs"));
    workbench.update_in(cx, |w, window, cx| w.open_project_file(alpha.clone(), window, cx));
    settle(cx, |cx| at(&workbench, cx) == Some((alpha.clone(), (0, 0))));
    for (path, line) in [(&alpha, 0), (&alpha, 2), (&beta, 0)] {
        let place = serde_json::json!({ "path": path.to_string_lossy(), "line": line });
        run_action(cx, "navigate.toggleBookmark", Some(place));
    }
    assert_eq!(workbench.read_with(cx, |w, _| w.bookmarks().data().items.len()), 3);

    // The caret is on the first bookmark: the one before it is the last one.
    run_action(cx, "navigate.prevBookmark", None);
    settle(cx, |cx| at(&workbench, cx) == Some((beta.clone(), (0, 0))));
    run_action(cx, "navigate.prevBookmark", None);
    settle(cx, |cx| at(&workbench, cx) == Some((alpha.clone(), (2, 0))));
    run_action(cx, "navigate.prevBookmark", None);
    settle(cx, |cx| at(&workbench, cx) == Some((alpha.clone(), (0, 0))));
    run_action(cx, "navigate.prevBookmark", None);
    settle(cx, |cx| at(&workbench, cx) == Some((beta.clone(), (0, 0))));
    close_backend_project(cx);
}

/// The Switcher opened backward, with Ctrl held, starts on the editor used longest ago; letting
/// go of Ctrl opens it.
#[gpui_kit::test]
async fn the_switcher_opened_backward_starts_on_the_oldest_editor_and_opens_it_when_ctrl_is_released(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    let showing = |cx: &mut VisualTestContext| at(&workbench, cx).map(|(path, _)| path);
    for name in ["src/alpha.rs", "src/beta.rs", "notes.txt"] {
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join(name), window, cx));
        settle(cx, |cx| showing(cx) == Some(root.join(name)));
    }

    cx.simulate_modifiers_change(Modifiers { control: true, ..Modifiers::default() });
    run_action(cx, "navigate.switcherBack", None);
    let switcher = workbench.read_with(cx, |w, _| w.recent_files()).expect("the Switcher is open");
    settle(cx, |cx| switcher.read_with(cx, |popup, _| !popup.is_loading()));
    assert!(switcher.read_with(cx, |popup, _| popup.is_switcher()));
    assert_eq!(switcher.read_with(cx, |popup, _| popup.lines()), ["notes.txt", "src/beta.rs", "src/alpha.rs"]);
    let alpha = root.join("src/alpha.rs").to_string_lossy().into_owned();
    assert_eq!(switcher.read_with(cx, |popup, _| popup.selected_file()), Some(alpha));

    cx.simulate_modifiers_change(Modifiers::default());
    settle(cx, |cx| showing(cx) == Some(root.join("src/alpha.rs")));
    assert!(workbench.read_with(cx, |w, _| w.recent_files().is_none()));
    close_backend_project(cx);
}
