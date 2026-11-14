use super::*;
use crate::project_view::file_dialogs::{FileDialog, RefactoringPreview};

/// A project where `README.md` mentions `docs/old.txt`, with the Rename dialog open on that file and
/// the new name `new.txt` typed.
fn rename_dialog_on_old_txt(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, Entity<FileDialog>, &mut VisualTestContext) {
    let (folder, root, workbench, cx) =
        trusted_project(cx, &[("docs/old.txt", "first\n"), ("README.md", "See old.txt for details.\n")]);
    select_in_tree(&workbench, &root.join("docs/old.txt"), cx);
    run_action(cx, "project.rename", None);
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>().is_some()));
    let dialog = workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>()).expect("the Rename dialog");
    let field = dialog.read_with(cx, |dialog, _| dialog.name_field().cloned()).expect("a name field");
    field.update(cx, |field, cx| field.set_text("new.txt", cx));
    cx.run_until_parked();
    (folder, root, workbench, dialog, cx)
}

/// With both searches unticked the Rename dialog renames the file at once: no preview, the file that
/// mentions the old name keeps it, and the result is announced.
#[gpui_kit::test]
async fn skips_the_search_without_options(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, dialog, cx) = rename_dialog_on_old_txt(cx);
    let (old, new, readme) = (root.join("docs/old.txt"), root.join("docs/new.txt"), root.join("README.md"));
    let boxes = |cx: &mut VisualTestContext| {
        let boxes = dialog.read_with(cx, |dialog, _| dialog.checkboxes());
        boxes.into_iter().map(|(id, _, on)| (id, on)).collect::<Vec<_>>()
    };
    assert_eq!(boxes(cx), [("references", true), ("text", true)]);
    click_on(cx, "file-dialog-references");
    click_on(cx, "file-dialog-text");
    assert_eq!(boxes(cx), [("references", false), ("text", false)]);
    click_on(cx, "file-dialog-ok");
    settle(cx, |cx| {
        (new.is_file() && !old.exists())
            || workbench.read_with(cx, |w, _| w.file_dialog::<RefactoringPreview>().is_some())
    });
    assert!(workbench.read_with(cx, |w, _| w.file_dialog::<RefactoringPreview>()).is_none(), "nothing to preview");
    assert!(new.is_file() && !old.exists());
    assert_eq!(std::fs::read_to_string(&readme).expect("read"), "See old.txt for details.\n");
    settle(cx, |cx| notification_titles(&workbench, cx).contains(&"Rename 'old.txt' to 'new.txt'".to_owned()));
    close_backend_project(cx);
}

/// Enter starts the rename with its searches and Escape, pressed before anything comes back, stops
/// it: once the refactoring lets go of its dialog, the file still has its old name, the file that
/// mentions it is unchanged and no preview opened.
#[gpui_kit::test]
async fn stops_when_cancelled_before_the_search_answers(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, dialog, cx) = rename_dialog_on_old_txt(cx);
    let (old, new, readme) = (root.join("docs/old.txt"), root.join("docs/new.txt"), root.join("README.md"));
    let released = dialog.downgrade();
    drop(dialog);
    let preview_open =
        |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.file_dialog::<RefactoringPreview>().is_some());
    cx.simulate_keystrokes("enter escape");
    settle(cx, |cx| released.upgrade().is_none() || preview_open(cx) || new.exists());
    assert!(!preview_open(cx), "the stopped search shows no preview");
    assert!(old.is_file() && !new.exists(), "the file keeps its name");
    assert_eq!(std::fs::read_to_string(&readme).expect("read"), "See old.txt for details.\n");
    assert!(workbench.read_with(cx, |w, _| w.file_dialog::<FileDialog>()).is_none(), "the dialog closed");
    close_backend_project(cx);
}
