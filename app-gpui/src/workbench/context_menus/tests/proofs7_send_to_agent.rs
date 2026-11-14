use super::*;
use crate::agents::store::HOME;

/// Send to Agent chosen in the editor's menu, opened inside a selection of two lines, mentions the file and those
/// lines in the prompt of the new task, and the menu closes.
#[gpui_kit::test]
async fn send_to_agent_chosen_in_the_editors_menu_mentions_the_selected_lines_in_the_new_tasks_prompt(
    cx: &mut TestAppContext,
) {
    let (f, cx) = open(cx, &[("a.txt", "one\ntwo\nthree\n")], Setup::default());
    let editor = f.open_file("a.txt", cx);
    let store = f.workbench.read_with(cx, |w, _| w.agents_store()).expect("the project has its agents");
    assert!(!store.read_with(cx, |store, _| store.composer(HOME).text.contains("@a.txt")));
    // "one\ntwo": lines 1 and 2.
    editor.update(cx, |editor, cx| editor.select(0, 7, cx));
    let at = at_offset(&editor, 2, cx);
    right_click(cx, at);
    let menu = f.editor_menu(cx).expect("the editor's menu is open");
    assert!(!disabled(&menu, "Send to Agent", cx));
    choose(&menu, &["Send to Agent"], cx);
    wait_long(cx, "the mention in the prompt", |cx| {
        store.read_with(cx, |store, _| store.composer(HOME).text.contains("@a.txt:1-2"))
    });
    assert!(f.editor_menu(cx).is_none(), "choosing the row closes the menu");
    let files = store.read_with(cx, |store, _| store.composer(HOME).files);
    assert!(files.iter().any(|file| file.rel == "a.txt"), "{files:?}");
}
