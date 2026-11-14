use super::*;
use crate::agents::store::HOME;

/// The file bookmarks of the window: (path, is a folder).
fn path_marks(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    workbench
        .read_with(cx, |w, _| w.bookmarks().data().paths.iter().map(|mark| (mark.path.clone(), mark.dir)).collect())
}

/// The rows of the root of the open Project view menu.
fn menu_rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).expect("a menu is open");
    let lines = menu.read_with(cx, |menu, _| menu.lines());
    let root = lines.first().map(Vec::as_slice).unwrap_or_default();
    root.iter().map(|row| row.trim_start_matches("> ").to_owned()).collect()
}

/// Add Bookmark in a folder's Project view menu bookmarks the folder as a folder; the menu then offers Remove
/// Bookmark, which removes it.
#[gpui_kit::test]
async fn add_bookmark_in_a_folders_project_view_menu_bookmarks_the_folder(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("src/a.txt", "a\n"), ("b.txt", "b\n")]);
    let src = root.join("src");
    select_in_tree(&workbench, &src, cx);
    right_click_row(&src, cx);
    assert!(menu_rows(&workbench, cx).contains(&"Add Bookmark".to_owned()), "{:?}", menu_rows(&workbench, cx));
    choose_in_menu(&workbench, &["Add Bookmark"], cx);
    wait_long(cx, "the folder bookmark", |cx| path_marks(&workbench, cx) == [(text_of(&src), true)]);

    right_click_row(&src, cx);
    assert!(menu_rows(&workbench, cx).contains(&"Remove Bookmark".to_owned()), "{:?}", menu_rows(&workbench, cx));
    choose_in_menu(&workbench, &["Remove Bookmark"], cx);
    wait_long(cx, "the folder bookmark to go", |cx| path_marks(&workbench, cx).is_empty());
    close_project(cx);
}

/// Send to Agent in a folder's Project view menu mentions the folder in the prompt of the new task.
#[gpui_kit::test]
async fn send_to_agent_in_a_folders_project_view_menu_mentions_the_folder_in_the_new_tasks_prompt(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("src/a.txt", "a\n"), ("b.txt", "b\n")]);
    let store = workbench.read_with(cx, |w, _| w.agents_store()).expect("the project has its agents");
    let src = root.join("src");
    select_in_tree(&workbench, &src, cx);
    right_click_row(&src, cx);
    choose_in_menu(&workbench, &["Send to Agent"], cx);
    wait_long(cx, "the mention in the prompt", |cx| {
        store.read_with(cx, |store, _| store.composer(HOME).files.iter().any(|file| file.rel == "src"))
    });
    let text = store.read_with(cx, |store, _| store.composer(HOME).text);
    assert!(text.contains("@src"), "{text}");
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).is_none(), "the menu closes");
    close_project(cx);
}
