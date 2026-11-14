use super::*;
use crate::platform::context::window_context;

/// The primary modifier and `key`, as this OS presses it.
fn primary(key: &str) -> String {
    let modifier = if crate::platform::keymap::is_mac_host() { "cmd" } else { "ctrl" };
    format!("{modifier}-{key}")
}

/// What the diff window shows, when it is open.
fn shown_diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<DiffData> {
    workbench.read_with(cx, |w, cx| w.diff_dialog().map(|dialog| dialog.read(cx).data().clone()))
}

/// Shows the Shelf tab with the shelf `id` opened on its files.
fn open_shelf(workbench: &Entity<Workbench>, id: &str, cx: &mut VisualTestContext) -> Entity<ShelfPanel> {
    run_action(cx, "vcs.showShelf", None);
    let panel = workbench.read_with(cx, |w, _| w.shelf_view().cloned()).expect("the Shelf tab");
    wait_long(cx, "the shelf is listed", |cx| !panel.update(cx, |p, cx| p.lines(cx)).is_empty());
    panel.update(cx, |p, cx| p.toggle(id, cx));
    cx.run_until_parked();
    panel
}

/// Copy as Patch to Clipboard chosen from the context menu of one file of a shelf with two puts that file's patch
/// on the clipboard and not the other's; the menu of a file offers the shelf's rows with Rename… disabled.
#[gpui_kit::test]
async fn copies_only_the_selected_file_patch_through_the_scoped_patch_command(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, two_changes);
    wait_for_changes(&workbench, 2, cx);
    let id = shelve_paths(&workbench, &[abs(&root, "a.txt"), abs(&root, "b.txt")], "Work", cx);
    open_shelf(&workbench, &id, cx);

    let row = cx.debug_bounds("shelf-row-a.txt").expect("the shelved file is drawn");
    cx.simulate_mouse_down(row.center(), MouseButton::Right, gpui_kit::Modifiers::default());
    cx.simulate_mouse_up(row.center(), MouseButton::Right, gpui_kit::Modifiers::default());
    wait_long(cx, "the file's menu", |cx| workbench.read_with(cx, |w, _| w.open_menu::<()>().is_some()));
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<()>()).expect("the menu is open");
    let lines: Vec<String> = menu.read_with(cx, |menu, _| menu.lines())[0]
        .iter()
        .map(|line| line.trim_start_matches("> ").to_owned())
        .collect();
    for label in [
        "Show Diff",
        "Show Diff in a New Tab",
        "Compare with Local",
        "Create Patch…",
        "Copy as Patch to Clipboard",
        "Import Patches…",
        "Delete…",
    ] {
        assert!(lines.iter().any(|line| line == label), "{label} in {lines:?}");
    }
    let rename =
        menu.read_with(cx, |menu, _| menu.rows().iter().find(|row| row.label == "Rename…").map(|row| row.disabled));
    assert_eq!(rename, Some(true), "a file has no name of its own to rename");

    let copy = lines.iter().position(|line| line == "Copy as Patch to Clipboard").expect("Copy as Patch is offered");
    let item: &'static str = Box::leak(format!("menu-row-0-{copy}").into_boxed_str());
    click_on(cx, item);
    wait_for_notice(&workbench, "Patch copied to the clipboard", cx);
    let copied = cx.read_from_clipboard().and_then(|item| item.text()).expect("the clipboard holds text");
    assert!(copied.contains("diff --git a/a.txt b/a.txt") && copied.contains("+one changed"), "{copied}");
    assert!(!copied.contains("b.txt"), "only the chosen file's patch is copied: {copied}");
}

/// Show Diff is bound to ⌘D on macOS and Ctrl+D elsewhere; pressed in the Shelf tab it shows the diff of the
/// shelved file selected there, and pressed in a text editor it leaves the diff alone and duplicates the line.
#[gpui_kit::test]
async fn routes_the_configured_show_diff_shortcut_to_the_focused_shelf_selection(cx: &mut TestAppContext) {
    use crate::platform::keymap::KeymapState;
    assert_eq!(KeymapState::new("intellij-macos").bindings_for("vcs.showDiff"), ["Meta+D"]);
    assert_eq!(KeymapState::new("intellij-windows").bindings_for("vcs.showDiff"), ["Ctrl+D"]);

    let (workbench, cx, _folders, root) = git_project(cx, two_changes);
    wait_for_changes(&workbench, 2, cx);
    let id = shelve_paths(&workbench, &[abs(&root, "a.txt")], "Work", cx);
    open_shelf(&workbench, &id, cx);
    click_on(cx, "shelf-row-a.txt");
    let truthy =
        |key: &str, cx: &mut VisualTestContext| cx.update(|window, cx| window_context(window, cx).is_truthy(key));
    assert!(truthy("shelfFocus", cx), "the Shelf tab has the keyboard");
    cx.simulate_keystrokes(&primary("d"));
    wait_long(cx, "the shelved diff", |cx| shown_diff(&workbench, cx).is_some());
    let data = shown_diff(&workbench, cx).expect("the diff is shown");
    assert_eq!(data.path, abs(&root, "a.txt"));
    assert_eq!(data.left, DiffSide::Text { text: "one\n".into(), label: "Shelved base".into() });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(shown_diff(&workbench, cx).is_none(), "Escape closes the diff");

    let file = root.join("a.txt");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    wait_long(cx, "the editor", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file))
    });
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    cx.update(|window, cx| {
        let focus = editor.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    assert!(truthy("editorTextFocus", cx), "the editor has the keyboard");
    cx.simulate_keystrokes(&primary("d"));
    cx.run_until_parked();
    assert!(shown_diff(&workbench, cx).is_none(), "the key shows no diff in a text editor");
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "one\none\n", "the key duplicates the line instead");
}
