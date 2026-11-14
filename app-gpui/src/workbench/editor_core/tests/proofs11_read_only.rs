use super::*;

/// Opens `path` and waits until its tab is the active one, whatever view shows it.
fn open_tab(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let path = path.to_path_buf();
    let before = active_tab(workbench, cx);
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
    settle(cx, |cx| active_tab(workbench, cx).is_some_and(|tab| Some(&tab) != before.as_ref()));
}

/// The id of the active tab of the active pane.
fn active_tab(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    workbench.read_with(cx, |w, _| {
        let tabs = w.tabs()?;
        tabs.pane(&tabs.active_pane)?.active.clone()
    })
}

/// Make File Read-only on `file` shown in the active tab asks nothing and leaves the file writable on disk, and the
/// text file opened before it, in its own tab, keeps its attribute too.
fn toggle_leaves_alone(workbench: &Entity<Workbench>, root: &Path, file: &Path, cx: &mut VisualTestContext) {
    let text = root.join("notes.txt");
    let editor = open(workbench, &text, cx);
    open_tab(workbench, file, cx);
    assert!(workbench.read_with(cx, |w, _| w.editor().is_none()), "no text editor shows the file");

    // The action needs a text editor (`textEditorActive`): the registry may refuse it here, which leaves the file
    // alone as well.
    let _refused = cx.update(|window, cx| execute_action_or_throw("file.toggleReadOnly", None, window, cx));
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()), "nothing is asked");
    assert!(!std::fs::metadata(file).expect("the file").permissions().readonly(), "the file stays writable");
    assert!(!std::fs::metadata(&text).expect("the file").permissions().readonly(), "the other file too");
    assert!(!editor.read_with(cx, |editor, _| editor.is_read_only()), "the other editor stays writable");
}

/// A binary file has no read-only attribute to toggle: Make File Read-only asks nothing and changes nothing.
#[gpui_kit::test]
async fn a_binary_file_is_left_alone_by_the_read_only_toggle(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "one\n")]);
    let file = root.join("data.bin");
    std::fs::write(&file, [0_u8, 1, 2, 0, 255, 0, 7, 0]).expect("the file is written");
    toggle_leaves_alone(&workbench, &root, &file, cx);
    close(cx);
}

/// A file too large for the text editor has no read-only attribute to toggle there: Make File Read-only asks
/// nothing and changes nothing.
#[gpui_kit::test]
async fn a_too_large_file_is_left_alone_by_the_read_only_toggle(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "one\n")]);
    let file = root.join("huge.log");
    {
        let mut out = std::fs::File::create(&file).expect("the file is made");
        std::io::Write::write_all(&mut out, "a line of a log\n".repeat(1024).as_bytes()).expect("the text is written");
        // Past the 50 MiB where the text editor gives up; the rest is a hole, nothing is written there.
        out.set_len(ide_vfs::READ_ONLY_THRESHOLD + 1024).expect("the file grows");
    }
    toggle_leaves_alone(&workbench, &root, &file, cx);
    close(cx);
}
