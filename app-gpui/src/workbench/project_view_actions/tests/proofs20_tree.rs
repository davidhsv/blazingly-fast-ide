//! Compare With… handing its files to the diff's actions, and Edit › Undo of the Project view's last move and
//! rename.

use super::*;
use crate::project_view::file_dialogs::FileDialog;

/// The actions the user runs from now on, in order, until the returned listener is removed.
fn record_actions(cx: &mut VisualTestContext) -> (Rc<RefCell<Vec<String>>>, u64) {
    let ran: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = ran.clone();
    let listener = cx.update(|_, cx| {
        cx.global_mut::<ActionRegistry>().on_action_executed(move |id, _, _, _| sink.borrow_mut().push(id.into()))
    });
    (ran, listener)
}

/// Compare With… on two files hands them to the diff's Compare Files, which shows them side by side; on one file
/// it hands the file to the diff's Compare With…, which asks for the other. It is the Project view's action: with
/// the editor's keyboard it is not available.
#[gpui_kit::test]
async fn compare_with_hands_two_files_to_compare_files_and_one_to_compare_with(cx: &mut TestAppContext) {
    let _home = start(cx);
    let files = [("a.txt", "one\ntwo\n"), ("b.txt", "one\nTWO\n")];
    let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    let diff_shows = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.diff_dialog().is_some());

    select_in_tree(&workbench, &a, cx);
    assert!(context_is_on(cx, "projectViewFocus"), "the Project view has the keyboard");
    let (ran, listener) = record_actions(cx);
    run_action(cx, "project.compareWith", Some(json!([text_of(&a), text_of(&b)])));
    wait_long(cx, "the two files side by side", diff_shows);
    wait_long(cx, "Compare Files to run", |_| ran.borrow().len() >= 2);
    assert_eq!(ran.borrow().as_slice(), ["project.compareWith", "diff.compareFiles"]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!diff_shows(cx));

    ran.borrow_mut().clear();
    select_in_tree(&workbench, &a, cx);
    run_action(cx, "project.compareWith", None);
    wait_long(cx, "the file to compare with is asked for", |cx| cx.did_prompt_for_paths());
    assert_eq!(ran.borrow().as_slice(), ["project.compareWith", "diff.compareWith"]);
    cx.simulate_path_prompt_response(move |_| Some(vec![b.clone()]));
    wait_long(cx, "the chosen file side by side", diff_shows);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|_, cx| cx.global_mut::<ActionRegistry>().remove_listener(listener));

    edit(&workbench, &a, cx);
    assert!(!context_is_on(cx, "projectViewFocus"));
    let refused = cx.update(|window, cx| execute_action_or_throw("project.compareWith", None, window, cx));
    assert!(refused.is_err(), "Compare With is the Project view's");
    assert!(!diff_shows(cx));
    close_project(cx);
}

/// Edit › Undo while the Project view has the keyboard takes back the view's last file operation: a move puts the
/// file back in its folder, then a rename gives it its old name back, on disk and in the tree.
#[gpui_kit::test]
async fn undo_in_the_project_view_takes_back_its_last_move_and_rename(cx: &mut TestAppContext) {
    let _home = start(cx);
    let files = [("src/data.json", "{}\n"), ("lib/keep.txt", "")];
    let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &files);
    let (data, moved) = (root.join("src/data.json"), root.join("lib/data.json"));
    let dialog_closed =
        |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>().is_none());

    select_in_tree(&workbench, &data, cx);
    run_action(cx, "project.move", None);
    let dialog = workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>()).expect("the Move dialog");
    let target = dialog.read_with(cx, |dialog, _| dialog.target_field().cloned()).expect("a target field");
    target.update(cx, |field, cx| field.set_text(&text_of(&root.join("lib")), cx));
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the move", |_| moved.is_file() && !data.exists());
    wait_long(cx, "the Move dialog to close", dialog_closed);

    select_in_tree(&workbench, &root.join("lib"), cx);
    assert!(context_is_on(cx, "projectViewFocus"));
    run_action(cx, "edit.undo", None);
    wait_long(cx, "the file to move back", |_| data.is_file() && !moved.exists());
    assert_eq!(std::fs::read_to_string(&data).expect("the file is back"), "{}\n");
    select_in_tree(&workbench, &data, cx);

    run_action(cx, "project.rename", None);
    wait_long(cx, "the Rename dialog", |cx| !dialog_closed(cx));
    let dialog = workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>()).expect("the Rename dialog");
    let name = dialog.read_with(cx, |dialog, _| dialog.name_field().cloned()).expect("a name field");
    name.update(cx, |field, cx| field.set_text("info.json", cx));
    cx.simulate_keystrokes("enter");
    let renamed = root.join("src/info.json");
    wait_long(cx, "the rename", |_| renamed.is_file() && !data.exists());
    wait_long(cx, "the Rename dialog to close", dialog_closed);

    select_in_tree(&workbench, &root.join("src"), cx);
    run_action(cx, "edit.undo", None);
    wait_long(cx, "the old name to come back", |_| data.is_file() && !renamed.exists());
    select_in_tree(&workbench, &data, cx);
    let tree = project_tree_of(&workbench, cx);
    wait_long(cx, "the tree to show the old name only", |cx| !tree_labels(&tree, cx).contains(&"info.json".to_owned()));
    close_project(cx);
}
