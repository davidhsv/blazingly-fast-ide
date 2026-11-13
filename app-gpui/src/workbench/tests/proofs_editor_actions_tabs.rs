use super::*;
use crate::platform::context::window_context;
use gpui_kit::Modifiers;

/// The names of the tabs of every pane, pane by pane.
fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    workbench.read_with(cx, |w, _| {
        let Some(tabs) = w.tabs() else { return Vec::new() };
        let ids: Vec<String> = tabs.pane_ids().into_iter().map(str::to_owned).collect();
        ids.iter().map(|id| w.tab_titles_of(id).into_iter().map(|tab| tab.name).collect()).collect()
    })
}

fn open_and_wait(workbench: &Entity<Workbench>, path: &std::path::Path, cx: &mut VisualTestContext) {
    let path = path.to_path_buf();
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path)));
}

/// The files whose documents the backend holds open for the window, by name, sorted.
fn open_documents(cx: &mut VisualTestContext) -> Vec<String> {
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    let Ok(project) = host.project("w1") else { return Vec::new() };
    let mut names: Vec<String> = project
        .documents
        .open_documents()
        .into_iter()
        .filter_map(|doc| project.documents.identity(doc).ok())
        .filter_map(|(path, _)| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

/// Close All Tabs closes the pinned tab with the others and leaves an empty editor area.
#[gpui_kit::test]
async fn close_all_tabs_closes_the_pinned_tab_with_the_others(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    open_and_wait(&workbench, &root.join("src/alpha.rs"), cx);
    run_action(cx, "editor.pinTab", None);
    open_and_wait(&workbench, &root.join("notes.txt"), cx);
    let pinned = workbench.read_with(cx, |w, _| {
        let tabs = w.tabs().expect("a project is open");
        let pane = tabs.pane(&tabs.active_pane).expect("the active pane");
        pane.tabs.iter().map(|tab| tab.pinned).collect::<Vec<_>>()
    });
    assert_eq!(pinned, [true, false]);

    run_action(cx, "editor.closeAllTabs", None);
    assert!(workbench.read_with(cx, |w, _| w.tab_titles().is_empty() && w.editor().is_none()));
    assert!(!cx.update(|window, cx| window_context(window, cx).is_truthy("editorTabsOpen")));
    close_backend_project(cx);
}

/// Close All Tabs closes the tabs of every pane, not only those of the active one, and forgets
/// them: Reopen Closed Tab has nothing to bring back afterwards.
#[gpui_kit::test]
async fn close_all_tabs_closes_every_pane_and_leaves_nothing_to_reopen(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    open_and_wait(&workbench, &root.join("src/alpha.rs"), cx);
    open_and_wait(&workbench, &root.join("notes.txt"), cx);
    let first = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    run_action(cx, "editor.splitRight", None);
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some_and(|editor| *editor != first)));
    assert_eq!(
        panes(&workbench, cx),
        [vec!["alpha.rs".to_owned(), "notes.txt".to_owned()], vec!["notes.txt".to_owned()]]
    );

    run_action(cx, "editor.closeAllTabs", None);
    assert!(panes(&workbench, cx).iter().all(Vec::is_empty), "every pane is empty: {:?}", panes(&workbench, cx));
    assert!(workbench.read_with(cx, |w, _| w.editor().is_none()));
    run_action(cx, "editor.reopenClosedTab", None);
    assert!(panes(&workbench, cx).iter().all(Vec::is_empty), "nothing is reopened: {:?}", panes(&workbench, cx));
    close_backend_project(cx);
}

/// A closed tab releases its document in the backend, unless another pane still shows the same
/// file: the document goes with the last tab that shows it.
#[gpui_kit::test]
async fn closing_a_tab_releases_its_document_once_no_other_tab_shows_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    open_and_wait(&workbench, &root.join("src/alpha.rs"), cx);
    open_and_wait(&workbench, &root.join("notes.txt"), cx);
    settle(cx, |cx| open_documents(cx) == ["alpha.rs", "notes.txt"]);

    // Two panes show notes.txt: one document for both. Only the editor's id is kept, so that the
    // test itself holds no editor alive.
    let first = workbench.read_with(cx, |w, _| w.editor().map(Entity::entity_id)).expect("an editor");
    run_action(cx, "editor.splitRight", None);
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some_and(|editor| editor.entity_id() != first)));
    assert_eq!(panes(&workbench, cx).len(), 2);
    assert_eq!(open_documents(cx), ["alpha.rs", "notes.txt"]);

    // Closing the new pane's tab leaves notes.txt open in the first pane.
    run_action(cx, "editor.closeTab", None);
    settle(cx, |cx| panes(&workbench, cx).iter().map(Vec::len).sum::<usize>() == 2);
    cx.run_until_parked();
    assert_eq!(open_documents(cx), ["alpha.rs", "notes.txt"]);

    // Closing its last tab releases it; alpha.rs stays.
    run_action(cx, "editor.closeTab", None);
    settle(cx, |cx| open_documents(cx) == ["alpha.rs"]);
    assert_eq!(panes(&workbench, cx).concat(), ["alpha.rs"]);
    run_action(cx, "editor.closeTab", None);
    settle(cx, |cx| open_documents(cx).is_empty());
    close_backend_project(cx);
}

/// Reload with Encoding, chosen from the encoding item's menu with nothing unsaved, reads the
/// file again from disk in that encoding: the editor shows the bytes decoded so, the item names
/// the encoding and the file has no unsaved changes.
#[gpui_kit::test]
async fn reload_with_encoding_reads_the_file_again_in_the_chosen_encoding(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("menu.txt", "café\n")]);
    open_and_wait(&workbench, &root.join("menu.txt"), cx);
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let items = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, cx| w.status_items(cx));
    let menu = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| w.open_menu::<StatusCommand>()).expect("the item's menu is open")
    };
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "café\n");
    assert_eq!(items(cx)[2], "UTF-8");

    cx.run_until_parked();
    let at = cx.debug_bounds("status:editor.encoding").expect("the status bar item is drawn").center();
    cx.simulate_click(at, Modifiers::default());
    let rows: Vec<(String, bool)> =
        menu(cx).read_with(cx, |menu, _| menu.rows().iter().map(|row| (row.label.clone(), row.disabled)).collect());
    assert_eq!(rows[0], ("Reload with Encoding".to_owned(), false), "nothing is unsaved: reloading is offered");
    cx.simulate_keystrokes("right");
    let offered: Vec<String> =
        menu(cx).read_with(cx, |menu, _| menu.rows().iter().map(|row| row.label.clone()).collect());
    let index = offered.iter().position(|label| label == "windows-1252").expect("windows-1252 is offered");
    for _ in 0..index {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    // The two UTF-8 bytes of "é" read as windows-1252 are "Ã©".
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "cafÃ©\n");
    assert_eq!(items(cx)[2], "windows-1252");
    assert!(!editor.read_with(cx, |e, _| e.is_dirty()));
    assert_eq!(std::fs::read(root.join("menu.txt")).expect("read"), "café\n".as_bytes(), "the file is unchanged");
    close_backend_project(cx);
}
