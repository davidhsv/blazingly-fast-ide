use super::*;
use crate::project_view::file_dialogs::FileDialog;
use gpui_kit::{ClipboardEntry, ClipboardItem, ExternalPaths};

/// Paste in the Project view of a file the file manager copied, in a local project, copies it into the selected
/// folder within the project's file system: the copy is on disk with the file's content, the original stays where
/// it was, and the tree lists the copy.
#[gpui_kit::test]
async fn uses_project_copy_for_a_local_destination(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("src/main.txt", "main\n")]);
    let outside = tempfile::tempdir().expect("a temporary folder");
    let original = outside.path().canonicalize().expect("the folder exists").join("notes.txt");
    std::fs::write(&original, "from the file manager\n").expect("the file is written");
    cx.write_to_clipboard(ClipboardItem {
        entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(vec![original.clone()].into()))],
    });

    let src = root.join("src");
    select_in_tree(&workbench, &src, cx);
    run_action(cx, "project.paste", None);
    // A single file may be offered under another name first: the name it has is kept.
    if workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>().is_some()) {
        cx.simulate_keystrokes("enter");
    }
    let copy = src.join("notes.txt");
    wait_long(cx, "the copy", |_| copy.is_file());
    assert_eq!(std::fs::read_to_string(&copy).expect("the copy is read"), "from the file manager\n");
    assert!(original.is_file(), "the original stays");
    let tree = project_tree_of(&workbench, cx);
    let at = text_of(&copy);
    wait_long(cx, "the tree lists the copy", |cx| {
        tree.update(cx, |tree, cx| tree.reveal_path(&at, cx))
            && tree.read_with(cx, |tree, _| tree.rows().iter().any(|row| row.node.path == at))
    });
    close_project(cx);
}
