use super::*;
use crate::find::popup_view::SEARCH_DEBOUNCE;
use crate::find::session::FindStatus;
use crate::find::store::ScopeTab;
use serde_json::json;

/// Waits for the popup's search to finish and returns its rows.
fn searched(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Vec<String> {
    for _ in 0..300 {
        cx.executor().advance_clock(SEARCH_DEBOUNCE);
        cx.run_until_parked();
        if popup.read_with(cx, |popup, _| popup.status() == FindStatus::Done && !popup.is_stale()) {
            return popup.read_with(cx, |popup, _| popup.lines());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the search did not finish within three seconds");
}

fn find_popup(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<FindPopup> {
    workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open")
}

/// The scope tab and the directory the open popup searches in.
fn scope(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> (ScopeTab, String) {
    popup.read_with(cx, |popup, _| (popup.options().scope_tab, popup.options().directory.clone()))
}

/// Replace in Files counts what it found, keeps marking the matches in the file shown once a
/// replacement is typed (the results show no diff unless the preview is switched to it), and
/// offers Replace All. Nothing is replaced on disk before that.
#[gpui_kit::test]
async fn streams_results_and_highlights_the_matches_while_replacing(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    run_action(cx, "find.replaceInPath", None);
    let popup = find_popup(&workbench, cx);
    assert!(popup.read_with(cx, |popup, _| popup.is_replace()));
    cx.simulate_input("needle");
    assert_eq!(searched(&popup, cx).len(), 1);
    assert_eq!(popup.update(cx, |popup, cx| popup.status_text(cx)), "1 match in 1 file");

    popup.update(cx, |popup, cx| popup.set_replacement("item", cx));
    let lines = searched(&popup, cx);
    assert!(lines.len() == 1 && lines[0].contains("a needle here"), "{lines:?}");
    let notes = text_of(&root.join("notes.txt"));
    wait_long(cx, "the preview of notes.txt", |cx| {
        popup.read_with(cx, |popup, _| popup.preview_path() == Some(notes.as_str()))
    });
    popup.read_with(cx, |popup, _| {
        assert_eq!(popup.preview_line(), Some((1, "  a needle here".to_owned())));
        assert_eq!(popup.preview_marks(1), vec![(4, 10)]);
        assert!(!popup.preview_mode());
        assert!(popup.diff_lines().is_empty(), "the matches are marked, not shown as a diff");
    });
    assert_eq!(popup.update(cx, |popup, cx| popup.replace_label(cx)), "Replace");
    cx.run_until_parked();
    assert!(cx.debug_bounds("find-popup-replace-all").is_some(), "Replace All is offered");
    assert_eq!(std::fs::read_to_string(root.join("notes.txt")).expect("read"), "first\n  a needle here\n");
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// The preview of the selected result marks every match of its file and is on the selected
/// result's line; moving the selection moves the preview's line with it.
#[gpui_kit::test]
async fn marks_the_matches_of_the_previewed_file_and_its_current_line(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("two.txt", "needle one\nskip\nneedle two needle\n")]);
    run_action(cx, "find.inPath", None);
    let popup = find_popup(&workbench, cx);
    cx.simulate_input("needle");
    let lines = searched(&popup, cx);
    assert_eq!(lines.len(), 2, "{lines:?}");
    let path = text_of(&root.join("two.txt"));
    wait_long(cx, "the preview of two.txt", |cx| {
        popup.read_with(cx, |popup, _| popup.preview_path() == Some(path.as_str()) && popup.preview_line().is_some())
    });
    popup.read_with(cx, |popup, _| {
        assert_eq!(popup.preview_line(), Some((0, "needle one".to_owned())));
        assert_eq!(popup.preview_marks(0), vec![(0, 6)]);
        assert_eq!(popup.preview_marks(1), Vec::<(u32, u32)>::new());
        assert_eq!(popup.preview_marks(2), vec![(0, 6), (11, 17)]);
    });
    cx.simulate_keystrokes("down");
    wait_long(cx, "the preview to follow the selection", |cx| {
        popup.read_with(cx, |popup, _| popup.preview_line() == Some((2, "needle two needle".to_owned())))
    });
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// Find and Replace in Files take the directory the action is given (`directory` before the older
/// `path`) with the query; an argument with only a query keeps the directory the popup has.
#[gpui_kit::test]
async fn keeps_explicit_directory_path_precedence_and_a_query_only_argument(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) =
        trusted_project(cx, &[("src/a.txt", "needle\n"), ("lib/b.txt", "needle\n"), ("docs/c.txt", "needle\n")]);
    let (src, lib, docs) = (text_of(&root.join("src")), text_of(&root.join("lib")), text_of(&root.join("docs")));
    for (id, replace) in [("find.inPath", false), ("find.replaceInPath", true)] {
        run_action(cx, id, Some(json!({ "directory": src, "path": docs, "query": "needle" })));
        let popup = find_popup(&workbench, cx);
        assert_eq!(popup.read_with(cx, |popup, _| popup.is_replace()), replace);
        assert_eq!(scope(&popup, cx), (ScopeTab::Directory, src.clone()));
        assert_eq!(popup.read_with(cx, |popup, _| popup.options().pattern.clone()), "needle");

        run_action(cx, id, Some(json!({ "path": lib })));
        assert_eq!(scope(&popup, cx), (ScopeTab::Directory, lib.clone()));

        run_action(cx, id, Some(json!({ "query": "other" })));
        assert_eq!(scope(&popup, cx), (ScopeTab::Directory, lib.clone()));
        assert_eq!(popup.read_with(cx, |popup, _| popup.options().pattern.clone()), "other");
        cx.simulate_keystrokes("escape");
        assert!(workbench.read_with(cx, |w, _| w.find_popup().is_none()));
    }
    close_backend_project(cx);
}

/// With the keyboard in the editor, Find and Replace in Files open on the scope the popup was
/// left on: the directory it was given, or the whole project once that tab was chosen.
#[gpui_kit::test]
async fn keeps_the_previous_scope_when_no_path_tree_owns_focus(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.txt", "needle\n")]);
    let file = root.join("src/a.txt");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file.as_path())));
    let src = text_of(&root.join("src"));
    let focus_editor = |cx: &mut VisualTestContext| {
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        cx.run_until_parked();
    };
    for id in ["find.inPath", "find.replaceInPath"] {
        run_action(cx, id, Some(json!({ "directory": src })));
        cx.simulate_keystrokes("escape");
        focus_editor(cx);
        run_action(cx, id, None);
        let popup = find_popup(&workbench, cx);
        assert_eq!(scope(&popup, cx), (ScopeTab::Directory, src.clone()));

        popup.update_in(cx, |popup, window, cx| popup.select_scope(ScopeTab::Project, window, cx));
        cx.simulate_keystrokes("escape");
        focus_editor(cx);
        run_action(cx, id, None);
        let popup = find_popup(&workbench, cx);
        assert_eq!(popup.read_with(cx, |popup, _| popup.options().scope_tab), ScopeTab::Project);
        cx.simulate_keystrokes("escape");
    }
    close_backend_project(cx);
}

/// A folder selected in the Project view, with the keyboard there, is the directory Find and
/// Replace in Files search in.
#[gpui_kit::test]
async fn takes_the_focused_directory_of_the_project_view(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.txt", "needle\n"), ("lib/b.txt", "needle\n")]);
    for (id, folder) in [("find.inPath", "src"), ("find.replaceInPath", "lib")] {
        let directory = root.join(folder);
        select_in_tree(&workbench, &directory, cx);
        run_action(cx, id, None);
        let popup = find_popup(&workbench, cx);
        assert_eq!(scope(&popup, cx), (ScopeTab::Directory, text_of(&directory)), "{id} from the Project view");
        cx.simulate_keystrokes("escape");
    }
    close_backend_project(cx);
}
